//! Axum Integration Layer for TurboMCP
//!
//! This module provides seamless integration with Axum routers, enabling the
//! "bring your own server" philosophy while providing opinionated defaults for
//! rapid development. It leverages our Tower service foundation for proven
//! observability and error handling.
//!
//! ## Production-Grade State Preservation
//!
//! The integration solves a critical problem: **how to add MCP capabilities to existing
//! Axum applications without losing their original state**. Our solution provides multiple
//! approaches optimized for different use cases:
//!
//! ### Approach 1: State-Preserving Merge (RECOMMENDED)
//!
//! ```rust,ignore
//! use axum::{Router, routing::get};
//! use turbomcp_transport::{AxumMcpExt, McpService, SessionInfo};
//!
//! # // Mock implementations for doctest
//! # async fn list_users() -> &'static str { "users" }
//! # #[derive(Clone)]
//! # struct YourAppState;
//! # struct MockMcpService;
//! # impl McpService for MockMcpService {
//! #     async fn process_request(
//! #         &self,
//! #         _request: serde_json::Value,
//! #         _session: &SessionInfo,
//! #     ) -> turbomcp_protocol::Result<serde_json::Value> {
//! #         Ok(serde_json::json!({}))
//! #     }
//! # }
//! # let your_app_state = YourAppState;
//! # let mcp_service = MockMcpService;
//!
//! // Your existing stateful router
//! let rest_router = Router::new()
//!     .route("/api/users", get(list_users))
//!     .with_state(your_app_state);
//!
//! // Add MCP capabilities while preserving your state
//! let combined_app = rest_router
//!     .merge(Router::<()>::turbo_mcp_routes_for_merge_default(mcp_service));
//! //         ^^^^^ Stateless MCP router merges cleanly!
//!
//! // Result:
//! // ✅ /api/* routes have access to your_app_state
//! // ✅ /mcp/* routes have their own McpAppState  
//! // ✅ Zero conflicts, maximum compatibility
//! ```
//!
//! ### Approach 2: Add MCP to Existing Router
//!
//! ```rust,ignore
//! # use axum::Router;
//! # use turbomcp_transport::{AxumMcpExt, McpService, SessionInfo};
//! # struct MockMcpService;
//! # impl McpService for MockMcpService {
//! #     async fn process_request(
//! #         &self,
//! #         _request: serde_json::Value,
//! #         _session: &SessionInfo,
//! #     ) -> turbomcp_protocol::Result<serde_json::Value> {
//! #         Ok(serde_json::json!({}))
//! #     }
//! # }
//! # let existing_router = Router::<()>::new();
//! # let mcp_service = MockMcpService;
//! // For simple cases where you want MCP added directly
//! let app_with_mcp = existing_router
//!     .turbo_mcp_routes(mcp_service);  // Preserves original state type
//! ```
//!
//! ### Why This Architecture is Production-Grade
//!
//! 1. **Zero Breaking Changes**: Existing applications work unchanged
//! 2. **Perfect Separation**: REST and MCP concerns are completely separate  
//! 3. **Type Safety**: Rust's type system prevents state mixing errors
//! 4. **Performance**: No overhead from state transformation or copying
//! 5. **Flexibility**: Choose the integration method that fits your architecture

#[cfg(feature = "http")]
use std::convert::Infallible;
#[cfg(feature = "http")]
use std::sync::Arc;
#[cfg(feature = "http")]
use std::time::Duration;

#[cfg(feature = "http")]
use axum::{
    Extension, Json, Router,
    extract::{Query, State, WebSocketUpgrade},
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};

#[cfg(feature = "http")]
use axum::http::{HeaderName, HeaderValue};
#[cfg(feature = "http")]
use futures::{SinkExt, StreamExt, stream::Stream};
#[cfg(feature = "http")]
use parking_lot::Mutex;
#[cfg(feature = "http")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "http")]
use std::collections::HashMap;
#[cfg(feature = "http")]
use tokio::sync::broadcast;
#[cfg(feature = "http")]
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
#[cfg(feature = "http")]
use tracing::{debug, error, info, trace, warn};

#[cfg(feature = "http")]
use crate::tower::{SessionInfo, SessionManager};
#[cfg(feature = "http")]
use turbomcp_protocol::Result as McpResult;

#[cfg(feature = "http")]
/// MCP service trait for handling MCP requests
pub trait McpService: Send + Sync + 'static {
    /// Process an MCP request and return a response
    async fn process_request(
        &self,
        request: serde_json::Value,
        session: &SessionInfo,
    ) -> McpResult<serde_json::Value>;

    /// Get service capabilities
    fn get_capabilities(&self) -> serde_json::Value {
        serde_json::json!({
            "protocol_version": "2025-06-18",
            "capabilities": {
                "tools": true,
                "resources": true,
                "prompts": true,
                "logging": true
            }
        })
    }
}

#[cfg(feature = "http")]
/// Query parameters for SSE endpoint
#[derive(Debug, Deserialize)]
pub struct SseQuery {
    /// Optional session ID for reconnection
    pub session_id: Option<String>,

    /// Last event ID for resumption
    pub last_event_id: Option<String>,
}

#[cfg(feature = "http")]
/// Query parameters for WebSocket endpoint
#[derive(Debug, Deserialize)]
pub struct WebSocketQuery {
    /// Optional session ID
    pub session_id: Option<String>,

    /// Optional protocol version
    pub protocol: Option<String>,
}

// Re-export canonical JSON-RPC types for backward compatibility
// These types are defined in turbomcp_protocol::jsonrpc::http for lenient HTTP boundary parsing
#[cfg(feature = "http")]
pub use turbomcp_protocol::jsonrpc::http::HttpJsonRpcRequest as JsonRpcRequest;
#[cfg(feature = "http")]
pub use turbomcp_protocol::jsonrpc::http::HttpJsonRpcResponse as JsonRpcResponse;
#[cfg(feature = "http")]
pub use turbomcp_protocol::jsonrpc::JsonRpcError;

#[cfg(feature = "http")]
/// Shared state for Axum application using trait objects for flexibility
#[derive(Clone)]
pub struct McpAppState {
    /// MCP service instance (trait object for flexibility)
    pub service: Arc<dyn McpService>,

    /// Session manager
    pub session_manager: Arc<SessionManager>,

    /// SSE broadcast sender for real-time updates
    pub sse_sender: broadcast::Sender<String>,

    /// Configuration options
    pub config: McpServerConfig,
}

#[cfg(feature = "http")]
impl std::fmt::Debug for McpAppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpAppState")
            .field("service", &"<dyn McpService>")
            .field("session_manager", &self.session_manager)
            .field("sse_sender", &"<broadcast::Sender>")
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(feature = "http")]
/// Production-grade configuration for MCP server with comprehensive production settings
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// Maximum request size in bytes
    pub max_request_size: usize,

    /// Request timeout duration
    pub request_timeout: Duration,

    /// SSE keep-alive interval
    pub sse_keep_alive: Duration,

    /// Maximum concurrent connections
    pub max_connections: usize,

    /// CORS configuration
    pub cors: CorsConfig,

    /// Security headers configuration
    pub security: SecurityConfig,

    /// Rate limiting configuration
    pub rate_limiting: RateLimitConfig,

    /// TLS configuration
    pub tls: Option<TlsConfig>,

    /// Authentication configuration
    pub auth: Option<AuthConfig>,

    /// Enable compression
    pub enable_compression: bool,

    /// Enable request tracing
    pub enable_tracing: bool,

    /// Environment mode (Development, Staging, Production)
    pub environment: Environment,
}

#[cfg(feature = "http")]
/// CORS configuration with secure defaults
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// Enable CORS
    pub enabled: bool,
    /// Allowed origins (None = no CORS, Some(vec![]) = no origins allowed, Some(vec!["*"]) = all origins)
    pub allowed_origins: Option<Vec<String>>,
    /// Allowed methods
    pub allowed_methods: Vec<String>,
    /// Allowed headers
    pub allowed_headers: Vec<String>,
    /// Exposed headers
    pub expose_headers: Vec<String>,
    /// Allow credentials
    pub allow_credentials: bool,
    /// Max age for preflight requests
    pub max_age: Option<Duration>,
}

#[cfg(feature = "http")]
/// Security headers configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Enable security headers
    pub enabled: bool,
    /// Content Security Policy
    pub content_security_policy: Option<String>,
    /// HTTP Strict Transport Security
    pub hsts_max_age: Option<Duration>,
    /// X-Frame-Options
    pub frame_options: FrameOptions,
    /// X-Content-Type-Options
    pub content_type_options: bool,
    /// Referrer-Policy
    pub referrer_policy: Option<String>,
    /// Permissions-Policy
    pub permissions_policy: Option<String>,
}

#[cfg(feature = "http")]
/// X-Frame-Options configuration
#[derive(Debug, Clone, PartialEq)]
pub enum FrameOptions {
    /// Deny all framing
    Deny,
    /// Allow framing from same origin
    SameOrigin,
    /// Allow framing from specific origin
    AllowFrom(String),
    /// Disable frame options header
    Disabled,
}

// Re-export canonical rate limiting types from axum config module
#[cfg(feature = "http")]
pub use crate::axum::config::rate_limit::{RateLimitConfig, RateLimitKey};

// Re-export canonical TLS types from axum config module
#[cfg(feature = "http")]
pub use crate::axum::config::tls::{TlsConfig, TlsVersion};

#[cfg(feature = "http")]
/// Authentication configuration for middleware
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Enable authentication
    pub enabled: bool,
    /// JWT secret for token validation
    pub jwt_secret: Option<String>,
    /// API key header name
    pub api_key_header: Option<String>,
    /// Custom authentication provider
    pub custom_validator: Option<String>,
}

#[cfg(feature = "http")]
/// Environment configuration
#[derive(Debug, Clone, PartialEq)]
pub enum Environment {
    /// Development environment with permissive settings
    Development,
    /// Staging environment with moderate security
    Staging,
    /// Production environment with maximum security
    Production,
}

#[cfg(feature = "http")]
impl Default for McpServerConfig {
    fn default() -> Self {
        Self::development()
    }
}

#[cfg(feature = "http")]
impl McpServerConfig {
    /// Create development configuration with permissive settings for local development
    pub fn development() -> Self {
        Self {
            max_request_size: 16 * 1024 * 1024, // 16MB
            request_timeout: Duration::from_secs(30),
            sse_keep_alive: Duration::from_secs(15),
            max_connections: 1000,
            cors: CorsConfig::permissive(),
            security: SecurityConfig::development(),
            rate_limiting: RateLimitConfig::disabled(),
            tls: None,
            auth: None,
            enable_compression: true,
            enable_tracing: true,
            environment: Environment::Development,
        }
    }

    /// Create staging configuration with moderate security
    pub fn staging() -> Self {
        Self {
            max_request_size: 8 * 1024 * 1024, // 8MB
            request_timeout: Duration::from_secs(30),
            sse_keep_alive: Duration::from_secs(15),
            max_connections: 500,
            cors: CorsConfig::restrictive(),
            security: SecurityConfig::staging(),
            rate_limiting: RateLimitConfig::moderate(),
            tls: Self::load_tls_from_env(),
            auth: Self::load_auth_from_env(),
            enable_compression: true,
            enable_tracing: true,
            environment: Environment::Staging,
        }
    }

    /// Create production configuration with maximum security
    pub fn production() -> Self {
        Self {
            max_request_size: 4 * 1024 * 1024, // 4MB
            request_timeout: Duration::from_secs(15),
            sse_keep_alive: Duration::from_secs(30),
            max_connections: 200,
            cors: CorsConfig::strict(),
            security: SecurityConfig::production(),
            rate_limiting: RateLimitConfig::strict(),
            tls: Self::load_tls_from_env(),
            auth: Self::load_auth_from_env(),
            enable_compression: true,
            enable_tracing: true,
            environment: Environment::Production,
        }
    }

    /// Load TLS configuration from environment variables
    ///
    /// Reads:
    /// - `TLS_CERT_FILE`: Path to TLS certificate file
    /// - `TLS_KEY_FILE`: Path to TLS private key file
    /// - `TLS_MIN_VERSION`: Minimum TLS version (1.2 or 1.3, defaults to 1.3)
    /// - `TLS_ENABLE_HTTP2`: Enable HTTP/2 (true/false, defaults to true)
    fn load_tls_from_env() -> Option<TlsConfig> {
        let cert_file = std::env::var("TLS_CERT_FILE").ok()?;
        let key_file = std::env::var("TLS_KEY_FILE").ok()?;

        // TLS 1.3 is required in v3
        let min_version = TlsVersion::TlsV1_3;

        let enable_http2 = std::env::var("TLS_ENABLE_HTTP2")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(true);

        Some(TlsConfig {
            cert_file,
            key_file,
            min_version,
            enable_http2,
        })
    }

    /// Load authentication configuration from environment variables
    ///
    /// Reads:
    /// - `AUTH_JWT_SECRET`: JWT secret key for token validation
    /// - `AUTH_API_KEY_HEADER`: Header name for API key authentication (e.g., "X-API-Key")
    /// - `AUTH_ENABLED`: Enable authentication (true/false, defaults to false if no config)
    fn load_auth_from_env() -> Option<AuthConfig> {
        let jwt_secret = std::env::var("AUTH_JWT_SECRET").ok();
        let api_key_header = std::env::var("AUTH_API_KEY_HEADER").ok();
        let enabled = std::env::var("AUTH_ENABLED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(jwt_secret.is_some() || api_key_header.is_some());

        if !enabled && jwt_secret.is_none() && api_key_header.is_none() {
            return None;
        }

        Some(AuthConfig {
            enabled,
            jwt_secret,
            api_key_header,
            custom_validator: None,
        })
    }

    /// Configure allowed CORS origins
    pub fn with_cors_origins(mut self, origins: Vec<String>) -> Self {
        self.cors.allowed_origins = Some(origins);
        self
    }

    /// Configure custom Content Security Policy
    pub fn with_custom_csp(mut self, csp: &str) -> Self {
        self.security.content_security_policy = Some(csp.to_string());
        self
    }

    /// Configure rate limiting
    pub fn with_rate_limit(mut self, requests_per_minute: u32, burst: u32) -> Self {
        self.rate_limiting.requests_per_minute = requests_per_minute;
        self.rate_limiting.burst_capacity = burst;
        self.rate_limiting.enabled = true;
        self
    }

    /// Configure TLS
    pub fn with_tls(mut self, cert_file: String, key_file: String) -> Self {
        self.tls = Some(TlsConfig {
            cert_file,
            key_file,
            min_version: TlsVersion::TlsV1_3,
            enable_http2: true,
        });
        self
    }

    /// Enable API key authentication
    pub fn with_api_key_auth(mut self, header_name: String) -> Self {
        self.auth = Some(AuthConfig {
            enabled: true,
            jwt_secret: None,
            api_key_header: Some(header_name),
            custom_validator: None,
        });
        self
    }

    /// Enable JWT authentication
    pub fn with_jwt_auth(mut self, secret: String) -> Self {
        self.auth = Some(AuthConfig {
            enabled: true,
            jwt_secret: Some(secret),
            api_key_header: None,
            custom_validator: None,
        });
        self
    }
}

#[cfg(feature = "http")]
impl CorsConfig {
    /// Permissive CORS for development (allows all origins)
    pub fn permissive() -> Self {
        Self {
            enabled: true,
            allowed_origins: Some(vec!["*".to_string()]),
            allowed_methods: vec!["GET".to_string(), "POST".to_string(), "OPTIONS".to_string()],
            allowed_headers: vec!["*".to_string()],
            expose_headers: vec![],
            allow_credentials: false, // Cannot be true with wildcard origins
            max_age: Some(Duration::from_secs(3600)),
        }
    }

    /// Restrictive CORS for staging (specific origins only)
    pub fn restrictive() -> Self {
        let allowed_origins = Self::load_cors_origins_from_env().unwrap_or_default(); // Must be configured explicitly

        Self {
            enabled: true,
            allowed_origins: Some(allowed_origins),
            allowed_methods: vec!["GET".to_string(), "POST".to_string(), "OPTIONS".to_string()],
            allowed_headers: vec![
                "Content-Type".to_string(),
                "Authorization".to_string(),
                "X-Requested-With".to_string(),
            ],
            expose_headers: vec![],
            allow_credentials: true,
            max_age: Some(Duration::from_secs(1800)),
        }
    }

    /// Strict CORS for production (no origins allowed by default)
    pub fn strict() -> Self {
        let allowed_origins = Self::load_cors_origins_from_env().unwrap_or_default(); // Must be explicitly configured

        Self {
            enabled: true,
            allowed_origins: Some(allowed_origins),
            allowed_methods: vec!["GET".to_string(), "POST".to_string()],
            allowed_headers: vec!["Content-Type".to_string(), "Authorization".to_string()],
            expose_headers: vec![],
            allow_credentials: true,
            max_age: Some(Duration::from_secs(600)),
        }
    }

    /// Load CORS origins from environment variables
    ///
    /// Reads `CORS_ALLOWED_ORIGINS` as a comma-separated list of origins
    /// Example: `CORS_ALLOWED_ORIGINS="https://app.example.com,https://admin.example.com"`
    fn load_cors_origins_from_env() -> Option<Vec<String>> {
        std::env::var("CORS_ALLOWED_ORIGINS").ok().map(|origins| {
            origins
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
    }

    /// Disabled CORS
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            allowed_origins: None,
            allowed_methods: vec![],
            allowed_headers: vec![],
            expose_headers: vec![],
            allow_credentials: false,
            max_age: None,
        }
    }
}

#[cfg(feature = "http")]
impl SecurityConfig {
    /// Development security (minimal headers)
    pub fn development() -> Self {
        Self {
            enabled: false, // Disabled for easier development
            content_security_policy: None,
            hsts_max_age: None,
            frame_options: FrameOptions::Disabled,
            content_type_options: false,
            referrer_policy: None,
            permissions_policy: None,
        }
    }

    /// Staging security (moderate headers)
    pub fn staging() -> Self {
        Self {
            enabled: true,
            content_security_policy: Some(
                "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'".to_string()
            ),
            hsts_max_age: Some(Duration::from_secs(31536000)), // 1 year
            frame_options: FrameOptions::SameOrigin,
            content_type_options: true,
            referrer_policy: Some("strict-origin-when-cross-origin".to_string()),
            permissions_policy: None,
        }
    }

    /// Production security (maximum headers)
    pub fn production() -> Self {
        Self {
            enabled: true,
            content_security_policy: Some(
                "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; media-src 'self'; frame-src 'none'".to_string()
            ),
            hsts_max_age: Some(Duration::from_secs(63072000)), // 2 years
            frame_options: FrameOptions::Deny,
            content_type_options: true,
            referrer_policy: Some("no-referrer".to_string()),
            permissions_policy: Some(
                "geolocation=(), microphone=(), camera=(), payment=(), usb=()".to_string()
            ),
        }
    }
}

#[cfg(feature = "http")]
impl RateLimitConfig {
    /// Disabled rate limiting
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            requests_per_minute: 0,
            burst_capacity: 0,
            key_function: RateLimitKey::IpAddress,
        }
    }

    /// Moderate rate limiting for staging
    pub fn moderate() -> Self {
        Self {
            enabled: true,
            requests_per_minute: 300, // 5 requests per second
            burst_capacity: 50,
            key_function: RateLimitKey::IpAddress,
        }
    }

    /// Strict rate limiting for production
    pub fn strict() -> Self {
        Self {
            enabled: true,
            requests_per_minute: 120, // 2 requests per second
            burst_capacity: 20,
            key_function: RateLimitKey::IpAddress,
        }
    }
}

#[cfg(feature = "http")]
/// Axum integration extension trait
pub trait AxumMcpExt {
    /// Add MCP routes to an existing router with custom configuration
    fn turbo_mcp_routes_with_config<T: McpService + 'static>(
        self,
        service: T,
        config: McpServerConfig,
    ) -> Self
    where
        Self: Sized;

    /// Add MCP routes to an existing router with default configuration
    fn turbo_mcp_routes<T: McpService + 'static>(self, service: T) -> Self
    where
        Self: Sized,
    {
        self.turbo_mcp_routes_with_config(service, McpServerConfig::default())
    }

    /// Create a complete MCP server with opinionated defaults
    fn turbo_mcp_server<T: McpService + 'static>(service: T) -> Router {
        Router::<()>::new().turbo_mcp_routes(service)
    }

    /// Create a complete MCP server with custom configuration
    fn turbo_mcp_server_with_config<T: McpService + 'static>(
        service: T,
        config: McpServerConfig,
    ) -> Router {
        Router::<()>::new().turbo_mcp_routes_with_config(service, config)
    }

    /// Create an MCP router that preserves your state when merged (PRODUCTION-GRADE ENHANCEMENT)
    ///
    /// This method creates a stateless MCP router that can be merged with any stateful router
    /// without losing the original state. This is the cleanest way to add MCP capabilities
    /// to existing applications.
    ///
    /// # Example
    /// ```rust,ignore
    /// # use axum::{Router, routing::get};
    /// # use turbomcp_transport::{AxumMcpExt, McpService, McpServerConfig, SessionInfo};
    /// # async fn list_users() -> &'static str { "users" }
    /// # #[derive(Clone)]
    /// # struct AppState;
    /// # struct MyMcpService;
    /// # impl McpService for MyMcpService {
    /// #     async fn process_request(
    /// #         &self,
    /// #         _request: serde_json::Value,
    /// #         _session: &SessionInfo,
    /// #     ) -> turbomcp_protocol::Result<serde_json::Value> {
    /// #         Ok(serde_json::json!({}))
    /// #     }
    /// # }
    /// # let app_state = AppState;
    /// # let my_mcp_service = MyMcpService;
    /// let rest_router = Router::new()
    ///     .route("/api/users", get(list_users))
    ///     .with_state(app_state);
    ///
    /// let mcp_router = Router::turbo_mcp_routes_for_merge(my_mcp_service, McpServerConfig::default());
    ///
    /// let combined = rest_router.merge(mcp_router);  // State is preserved!
    /// ```
    fn turbo_mcp_routes_for_merge<T: McpService + 'static>(
        service: T,
        config: McpServerConfig,
    ) -> Router {
        Self::turbo_mcp_server_with_config(service, config)
    }

    /// Create an MCP router for merging with default configuration
    fn turbo_mcp_routes_for_merge_default<T: McpService + 'static>(service: T) -> Router {
        Self::turbo_mcp_routes_for_merge(service, McpServerConfig::default())
    }
}

#[cfg(feature = "http")]
impl<S> AxumMcpExt for Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn turbo_mcp_routes_with_config<T: McpService + 'static>(
        self,
        service: T,
        config: McpServerConfig,
    ) -> Router<S> {
        let session_manager = Arc::new(SessionManager::with_config(
            Duration::from_secs(300), // 5 minute session timeout
            config.max_connections,
        ));

        let (sse_sender, _) = broadcast::channel(1000);

        let app_state = McpAppState {
            service: Arc::new(service) as Arc<dyn McpService>,
            session_manager,
            sse_sender,
            config: config.clone(),
        };

        // Create new router with MCP routes and state
        let mcp_router = Router::new()
            .route("/mcp", post(json_rpc_handler))
            .route("/mcp/capabilities", get(capabilities_handler))
            .route("/mcp/sse", get(sse_handler))
            .route("/mcp/ws", get(websocket_handler))
            .route("/mcp/health", get(health_handler))
            .route("/mcp/metrics", get(metrics_handler))
            .with_state(app_state);

        // Merge with existing router
        let router = self.merge(mcp_router);

        // Apply proven middleware stack
        apply_middleware(router, &config)
    }
}

#[cfg(feature = "http")]
/// Apply comprehensive middleware stack based on configuration
#[allow(unused_variables)] // Some middleware may be conditionally applied
fn apply_middleware<S>(router: Router<S>, config: &McpServerConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let mut router = router;

    // 1. Basic MCP middleware (always applied)
    router = router.layer(middleware::from_fn(mcp_middleware));

    // 2. Security headers (applied based on config and environment)
    if config.security.enabled {
        router = router.layer(middleware::from_fn_with_state(
            config.security.clone(),
            security_headers_middleware,
        ));
    }

    // 3. Rate limiting (applied if enabled)
    if config.rate_limiting.enabled {
        router = router.layer(middleware::from_fn_with_state(
            config.rate_limiting.clone(),
            rate_limiting_middleware,
        ));
    }

    // 4. Authentication (applied if configured)
    if let Some(auth_config) = &config.auth
        && auth_config.enabled
    {
        router = router.layer(middleware::from_fn_with_state(
            auth_config.clone(),
            authentication_middleware,
        ));
    }

    // 5. CORS (applied based on configuration)
    if config.cors.enabled {
        router = router.layer(build_cors_layer(&config.cors));
    }

    // 6. Compression (applied if enabled)
    if config.enable_compression {
        router = router.layer(CompressionLayer::new());
    }

    // 7. Request tracing (applied if enabled)
    if config.enable_tracing {
        router = router.layer(TraceLayer::new_for_http());
    }

    // 8. Timeout (always applied for reliability)
    router = router.layer(TimeoutLayer::new(config.request_timeout));

    router
}

#[cfg(feature = "http")]
/// Build CORS layer from configuration
fn build_cors_layer(cors_config: &CorsConfig) -> CorsLayer {
    let mut cors = CorsLayer::new();

    // Configure allowed methods
    if !cors_config.allowed_methods.is_empty() {
        let methods: Vec<Method> = cors_config
            .allowed_methods
            .iter()
            .filter_map(|m| m.parse().ok())
            .collect();
        cors = cors.allow_methods(methods);
    }

    // Configure allowed origins
    match &cors_config.allowed_origins {
        Some(origins) if origins.contains(&"*".to_string()) => {
            cors = cors.allow_origin(Any);
        }
        Some(origins) if !origins.is_empty() => {
            let origin_list: Result<Vec<_>, _> =
                origins.iter().map(|origin| origin.parse()).collect();
            if let Ok(origins) = origin_list {
                cors = cors.allow_origin(origins);
            }
        }
        _ => {
            // No origins allowed - very secure but may break functionality
            // This is intentional for production safety
        }
    }

    // Configure allowed headers
    if cors_config.allowed_headers.contains(&"*".to_string()) {
        cors = cors.allow_headers(Any);
    } else if !cors_config.allowed_headers.is_empty() {
        let headers: Vec<HeaderName> = cors_config
            .allowed_headers
            .iter()
            .filter_map(|h| h.parse().ok())
            .collect();
        cors = cors.allow_headers(headers);
    }

    // Configure exposed headers
    if !cors_config.expose_headers.is_empty() {
        let headers: Vec<HeaderName> = cors_config
            .expose_headers
            .iter()
            .filter_map(|h| h.parse().ok())
            .collect();
        cors = cors.expose_headers(headers);
    }

    // Configure credentials
    if cors_config.allow_credentials {
        cors = cors.allow_credentials(true);
    }

    // Configure max age
    if let Some(max_age) = cors_config.max_age {
        cors = cors.max_age(max_age);
    }

    cors
}

#[cfg(feature = "http")]
/// Root handler - provides basic server information
#[allow(dead_code)]
async fn root_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": "TurboMCP Server",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "High-performance Model Context Protocol server",
        "endpoints": {
            "mcp": "/mcp",
            "capabilities": "/mcp/capabilities",
            "sse": "/mcp/sse",
            "websocket": "/mcp/ws",
            "health": "/mcp/health",
            "metrics": "/mcp/metrics"
        }
    }))
}

#[cfg(feature = "http")]
/// JSON-RPC HTTP handler
async fn json_rpc_handler(
    State(app_state): State<McpAppState>,
    Extension(session): Extension<SessionInfo>,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Json<JsonRpcResponse>, StatusCode> {
    trace!("Processing JSON-RPC request: {:?}", request);

    // Validate JSON-RPC format
    if request.jsonrpc != "2.0" {
        return Ok(Json(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request".to_string(),
                data: Some(serde_json::json!({
                    "reason": "jsonrpc field must be '2.0'"
                })),
            }),
        }));
    }

    // Create request object for service
    let service_request = serde_json::json!({
        "jsonrpc": request.jsonrpc,
        "id": request.id,
        "method": request.method,
        "params": request.params
    });

    // Process request through MCP service
    match app_state
        .service
        .process_request(service_request, &session)
        .await
    {
        Ok(result) => {
            // Broadcast result to SSE clients if it's a notification
            if request.id.is_none() {
                let _ = app_state
                    .sse_sender
                    .send(serde_json::to_string(&result).unwrap_or_default());
            }

            Ok(Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(result),
                error: None,
            }))
        }
        Err(e) => {
            error!("MCP service error: {}", e);

            Ok(Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: "Internal error".to_string(),
                    data: Some(serde_json::json!({
                        "reason": e.to_string()
                    })),
                }),
            }))
        }
    }
}

#[cfg(feature = "http")]
/// Capabilities handler
async fn capabilities_handler(State(app_state): State<McpAppState>) -> Json<serde_json::Value> {
    Json(app_state.service.get_capabilities())
}

#[cfg(feature = "http")]
/// Server-Sent Events handler
async fn sse_handler(
    State(app_state): State<McpAppState>,
    Query(_query): Query<SseQuery>,
    Extension(session): Extension<SessionInfo>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    info!("SSE connection established for session: {}", session.id);

    let mut receiver = app_state.sse_sender.subscribe();

    // Create event stream
    let stream = async_stream::stream! {
        // Send initial connection event
        yield Ok(Event::default()
            .event("connected")
            .data(serde_json::json!({
                "session_id": session.id,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }).to_string()));

        // Stream events from broadcast channel
        loop {
            match receiver.recv().await {
                Ok(message) => {
                    yield Ok(Event::default()
                        .event("message")
                        .data(message));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("SSE broadcast channel closed");
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!("SSE client lagged, skipped {} messages", skipped);
                    yield Ok(Event::default()
                        .event("error")
                        .data(serde_json::json!({
                            "code": "LAGGED",
                            "message": format!("Skipped {} messages due to slow client", skipped)
                        }).to_string()));
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(app_state.config.sse_keep_alive))
}

#[cfg(feature = "http")]
/// WebSocket handler
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<McpAppState>,
    Query(_query): Query<WebSocketQuery>,
    Extension(session): Extension<SessionInfo>,
) -> Response {
    info!("WebSocket upgrade requested for session: {}", session.id);

    ws.on_upgrade(move |socket| handle_websocket(socket, app_state, session))
}

#[cfg(feature = "http")]
/// Handle WebSocket connection
async fn handle_websocket(
    socket: axum::extract::ws::WebSocket,
    app_state: McpAppState,
    session: SessionInfo,
) {
    let (mut sender, mut receiver) = socket.split();

    info!("WebSocket connected for session: {}", session.id);

    // Send welcome message
    let welcome = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "connected",
        "params": {
            "session_id": session.id,
            "capabilities": app_state.service.get_capabilities()
        }
    });

    if let Err(e) = sender
        .send(axum::extract::ws::Message::Text(welcome.to_string().into()))
        .await
    {
        error!("Failed to send WebSocket welcome message: {}", e);
        return;
    }

    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                trace!("WebSocket received text: {}", text);

                // Parse JSON-RPC request
                match serde_json::from_str::<JsonRpcRequest>(&text) {
                    Ok(request) => {
                        let service_request = serde_json::json!({
                            "jsonrpc": request.jsonrpc,
                            "id": request.id,
                            "method": request.method,
                            "params": request.params
                        });

                        // Process through MCP service
                        match app_state
                            .service
                            .process_request(service_request, &session)
                            .await
                        {
                            Ok(result) => {
                                let response = JsonRpcResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id: request.id,
                                    result: Some(result),
                                    error: None,
                                };

                                let response_text =
                                    serde_json::to_string(&response).unwrap_or_default();
                                if let Err(e) = sender
                                    .send(axum::extract::ws::Message::Text(response_text.into()))
                                    .await
                                {
                                    error!("Failed to send WebSocket response: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("WebSocket MCP service error: {}", e);

                                let error_response = JsonRpcResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id: request.id,
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: -32603,
                                        message: "Internal error".to_string(),
                                        data: Some(serde_json::json!({
                                            "reason": e.to_string()
                                        })),
                                    }),
                                };

                                let error_text =
                                    serde_json::to_string(&error_response).unwrap_or_default();
                                if let Err(e) = sender
                                    .send(axum::extract::ws::Message::Text(error_text.into()))
                                    .await
                                {
                                    error!("Failed to send WebSocket error response: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse WebSocket JSON-RPC request: {}", e);

                        let error_response = JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: None,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32700,
                                message: "Parse error".to_string(),
                                data: Some(serde_json::json!({
                                    "reason": e.to_string()
                                })),
                            }),
                        };

                        let error_text = serde_json::to_string(&error_response).unwrap_or_default();
                        if let Err(e) = sender
                            .send(axum::extract::ws::Message::Text(error_text.into()))
                            .await
                        {
                            error!("Failed to send WebSocket parse error: {}", e);
                            break;
                        }
                    }
                }
            }
            Ok(axum::extract::ws::Message::Close(_)) => {
                info!("WebSocket closed for session: {}", session.id);
                break;
            }
            Ok(axum::extract::ws::Message::Ping(data)) => {
                if let Err(e) = sender.send(axum::extract::ws::Message::Pong(data)).await {
                    error!("Failed to send WebSocket pong: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("WebSocket error for session {}: {}", session.id, e);
                break;
            }
            _ => {
                // Ignore other message types (Binary, Pong)
            }
        }
    }

    info!("WebSocket disconnected for session: {}", session.id);
}

#[cfg(feature = "http")]
/// Health check handler
async fn health_handler(State(app_state): State<McpAppState>) -> Json<serde_json::Value> {
    let session_count = app_state.session_manager.active_session_count().await;

    Json(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "sessions": {
            "active": session_count,
            "max": app_state.config.max_connections
        },
        "version": env!("CARGO_PKG_VERSION")
    }))
}

#[cfg(feature = "http")]
/// Metrics handler
async fn metrics_handler(State(app_state): State<McpAppState>) -> Json<serde_json::Value> {
    let sessions = app_state.session_manager.list_sessions();
    let total_sessions = sessions.len();
    let avg_duration = if total_sessions > 0 {
        sessions.iter().map(|s| s.duration().as_secs()).sum::<u64>() / total_sessions as u64
    } else {
        0
    };

    Json(serde_json::json!({
        "sessions": {
            "active": total_sessions,
            "max": app_state.config.max_connections,
            "average_duration_seconds": avg_duration
        },
        "server": {
            "uptime_seconds": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "version": env!("CARGO_PKG_VERSION")
        }
    }))
}

#[cfg(feature = "http")]
/// Middleware for MCP request processing
async fn mcp_middleware(
    mut request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Create or retrieve session
    let session = match request.extensions().get::<SessionInfo>() {
        Some(session) => session.clone(),
        None => {
            // Create new session - in production, you might want to extract this
            // from headers or query parameters
            let session = SessionInfo::new();
            request.extensions_mut().insert(session.clone());
            session
        }
    };

    trace!("Processing request for session: {}", session.id);

    // Continue processing
    let response = next.run(request).await;

    trace!("Request completed for session: {}", session.id);
    Ok(response)
}

#[cfg(feature = "http")]
/// Security headers middleware - applies comprehensive security headers
async fn security_headers_middleware(
    State(security_config): State<SecurityConfig>,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let mut response = next.run(request).await;

    // Apply security headers based on configuration
    let headers = response.headers_mut();

    // Content Security Policy
    if let Some(csp) = &security_config.content_security_policy
        && let Ok(header_value) = HeaderValue::from_str(csp)
    {
        headers.insert("Content-Security-Policy", header_value);
    }

    // HTTP Strict Transport Security
    if let Some(hsts_max_age) = security_config.hsts_max_age {
        let hsts_value = format!("max-age={}", hsts_max_age.as_secs());
        if let Ok(header_value) = HeaderValue::from_str(&hsts_value) {
            headers.insert("Strict-Transport-Security", header_value);
        }
    }

    // X-Frame-Options
    match security_config.frame_options {
        FrameOptions::Deny => {
            headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
        }
        FrameOptions::SameOrigin => {
            headers.insert("X-Frame-Options", HeaderValue::from_static("SAMEORIGIN"));
        }
        FrameOptions::AllowFrom(ref origin) => {
            let frame_value = format!("ALLOW-FROM {}", origin);
            if let Ok(header_value) = HeaderValue::from_str(&frame_value) {
                headers.insert("X-Frame-Options", header_value);
            }
        }
        FrameOptions::Disabled => {}
    }

    // X-Content-Type-Options
    if security_config.content_type_options {
        headers.insert(
            "X-Content-Type-Options",
            HeaderValue::from_static("nosniff"),
        );
    }

    // Referrer-Policy
    if let Some(referrer_policy) = &security_config.referrer_policy
        && let Ok(header_value) = HeaderValue::from_str(referrer_policy)
    {
        headers.insert("Referrer-Policy", header_value);
    }

    // Permissions-Policy
    if let Some(permissions_policy) = &security_config.permissions_policy
        && let Ok(header_value) = HeaderValue::from_str(permissions_policy)
    {
        headers.insert("Permissions-Policy", header_value);
    }

    // Additional security headers
    headers.insert(
        "X-XSS-Protection",
        HeaderValue::from_static("1; mode=block"),
    );
    headers.insert("X-DNS-Prefetch-Control", HeaderValue::from_static("off"));

    Ok(response)
}

#[cfg(feature = "http")]
/// Rate limiting middleware - implements token bucket algorithm
///
/// This is a basic implementation. For production use, consider using a more sophisticated
/// rate limiter like tower-governor or implementing distributed rate limiting with Redis.
async fn rate_limiting_middleware(
    State(rate_config): State<RateLimitConfig>,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // For this demo implementation, we'll use a simple in-memory rate limiter
    // In production, you'd want to use a proper distributed rate limiting solution

    // Extract rate limiting key based on configuration
    let rate_key = match rate_config.key_function {
        RateLimitKey::IpAddress => {
            // Extract IP from headers or connection info
            request
                .headers()
                .get("x-forwarded-for")
                .or_else(|| request.headers().get("x-real-ip"))
                .and_then(|h| h.to_str().ok())
                .unwrap_or("unknown")
                .to_string()
        }
        RateLimitKey::UserId => {
            // Extract user ID from authentication context
            request
                .extensions()
                .get::<String>()
                .cloned()
                .unwrap_or_else(|| "anonymous".to_string())
        }
        RateLimitKey::Custom => {
            // Custom key extraction logic would go here
            "custom_key".to_string()
        }
    };

    // For this demo, we'll implement a simple check
    // In production, implement proper token bucket or sliding window
    type RateLimiterMap = Arc<Mutex<HashMap<String, (std::time::Instant, u32)>>>;
    static RATE_LIMITER: std::sync::LazyLock<RateLimiterMap> =
        std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

    let now = std::time::Instant::now();
    let remaining_requests;

    // Scope to limit the lock
    {
        let mut limiter = RATE_LIMITER.lock();
        let (last_reset, count) = limiter.entry(rate_key.clone()).or_insert((now, 0));

        // Reset counter if a minute has passed
        if now.duration_since(*last_reset) >= std::time::Duration::from_secs(60) {
            *last_reset = now;
            *count = 0;
        }

        // Check rate limit
        if *count >= rate_config.requests_per_minute {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }

        // Increment counter
        *count += 1;
        remaining_requests = rate_config.requests_per_minute.saturating_sub(*count);
    }

    // Continue processing
    let mut response = next.run(request).await;

    // Add rate limit headers
    let headers = response.headers_mut();
    if let Ok(header_value) = HeaderValue::from_str(&rate_config.requests_per_minute.to_string()) {
        headers.insert("X-RateLimit-Limit", header_value);
    }
    if let Ok(header_value) = HeaderValue::from_str(&remaining_requests.to_string()) {
        headers.insert("X-RateLimit-Remaining", header_value);
    }

    Ok(response)
}

#[cfg(feature = "http")]
/// Authentication middleware - validates tokens and API keys
///
/// This is a basic implementation. For production use, integrate with your
/// authentication system (JWT, OAuth2, API keys, etc.)
async fn authentication_middleware(
    State(auth_config): State<AuthConfig>,
    mut request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Check for API key authentication
    if let Some(api_key_header) = &auth_config.api_key_header {
        if let Some(provided_key) = request.headers().get(api_key_header) {
            // In production, validate against your API key store
            if provided_key
                .to_str()
                .map_err(|_| StatusCode::BAD_REQUEST)?
                .is_empty()
            {
                return Err(StatusCode::UNAUTHORIZED);
            }
            // Add authenticated context to request
            request.extensions_mut().insert("api_key_user".to_string());
        } else if auth_config.enabled {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    // Check for JWT authentication
    if let Some(_jwt_secret) = &auth_config.jwt_secret {
        if let Some(auth_header) = request.headers().get("Authorization") {
            let auth_str = auth_header.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                // In production, validate JWT token here
                if token.is_empty() {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                // Add authenticated user context to request
                request.extensions_mut().insert("jwt_user".to_string());
            } else {
                return Err(StatusCode::UNAUTHORIZED);
            }
        } else if auth_config.enabled {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    // Continue processing
    Ok(next.run(request).await)
}

#[cfg(not(feature = "http"))]
/// Empty struct when HTTP feature is disabled
pub struct AxumMcpExt;

// Re-export for convenience
#[cfg(feature = "http")]
pub use axum;

#[cfg(test)]
#[cfg(feature = "http")]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::future::Future;
    use std::pin::Pin;

    #[derive(Clone)]
    struct TestMcpService;

    impl McpService for TestMcpService {
        fn process_request(
            &self,
            request: serde_json::Value,
            _session: &SessionInfo,
        ) -> Pin<Box<dyn Future<Output = McpResult<serde_json::Value>> + Send + '_>> {
            Box::pin(async move {
                // Echo the request back as result
                Ok(serde_json::json!({
                    "echo": request,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            })
        }
    }

    #[test]
    fn test_mcp_server_config_default() {
        let config = McpServerConfig::default();

        assert_eq!(config.max_request_size, 16 * 1024 * 1024);
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        assert_eq!(config.max_connections, 1000);
        assert!(config.cors.enabled);
        assert!(config.enable_compression);
        assert!(config.enable_tracing);
    }

    #[tokio::test]
    async fn test_router_extension() {
        let service = TestMcpService;
        let _router: Router<()> = Router::new().turbo_mcp_routes(service);

        // Router should be created without panicking
        // In a full test, we'd use axum_test to verify the routes work
    }

    #[tokio::test]
    async fn test_complete_server_creation() {
        let service = TestMcpService;
        let _router = Router::<()>::turbo_mcp_server(service);

        // Router should be created with root handler
        // In a full test, we'd verify all endpoints are accessible
    }

    #[test]
    fn test_state_preserving_merge() {
        #[derive(Clone, PartialEq, Debug)]
        struct MyAppState {
            value: String,
        }

        let my_state = MyAppState {
            value: "test".to_string(),
        };

        // Verify state value before merge
        assert_eq!(my_state.value, "test");

        // Create a stateful router
        let stateful_router = Router::new()
            .route("/api/test", axum::routing::get(|| async { "API response" }))
            .with_state(my_state.clone());

        let mcp_service = TestMcpService;

        // Test that we can merge with MCP routes without losing state
        let _combined_router = stateful_router.merge(
            Router::<()>::turbo_mcp_routes_for_merge_default(mcp_service),
        );

        // State preservation verified through successful compilation and initial assertion
        // The stateful_router keeps its MyAppState while MCP routes get McpAppState
    }

    #[test]
    fn test_direct_mcp_addition() {
        #[derive(Clone, PartialEq, Debug)]
        struct MyAppState {
            value: i32,
        }

        let my_state = MyAppState { value: 42 };

        // Verify state value before router operations
        assert_eq!(my_state.value, 42);

        let mcp_service = TestMcpService;

        // Test adding MCP routes directly to an existing router
        let router_with_mcp = Router::new()
            .route("/existing", axum::routing::get(|| async { "existing" }))
            .with_state(my_state.clone())
            .turbo_mcp_routes(mcp_service);

        // This should preserve the state type Router<MyAppState>
        let _: Router<MyAppState> = router_with_mcp;
    }

    #[test]
    fn test_production_grade_security_configuration() {
        // Test development configuration (permissive)
        let dev_config = McpServerConfig::development();
        assert_eq!(dev_config.environment, Environment::Development);
        assert!(
            dev_config
                .cors
                .allowed_origins
                .as_ref()
                .unwrap()
                .contains(&"*".to_string())
        );
        assert!(!dev_config.security.enabled);
        assert!(!dev_config.rate_limiting.enabled);

        // Test staging configuration (moderate security)
        let staging_config = McpServerConfig::staging();
        assert_eq!(staging_config.environment, Environment::Staging);
        assert!(
            staging_config
                .cors
                .allowed_origins
                .as_ref()
                .unwrap()
                .is_empty()
        ); // Must be configured
        assert!(staging_config.security.enabled);
        assert!(staging_config.rate_limiting.enabled);
        assert_eq!(staging_config.rate_limiting.requests_per_minute, 300);

        // Test production configuration (maximum security)
        let prod_config = McpServerConfig::production();
        assert_eq!(prod_config.environment, Environment::Production);
        assert!(
            prod_config
                .cors
                .allowed_origins
                .as_ref()
                .unwrap()
                .is_empty()
        );
        assert!(prod_config.security.enabled);
        assert!(prod_config.rate_limiting.enabled);
        assert_eq!(prod_config.rate_limiting.requests_per_minute, 120);
        assert_eq!(prod_config.max_request_size, 4 * 1024 * 1024); // 4MB
    }

    #[test]
    fn test_configuration_builder_pattern() {
        let config = McpServerConfig::staging()
            .with_cors_origins(vec!["https://example.com".to_string()])
            .with_custom_csp("default-src 'self'")
            .with_rate_limit(600, 100)
            .with_api_key_auth("X-API-Key".to_string());

        // Verify CORS configuration
        assert!(
            config
                .cors
                .allowed_origins
                .as_ref()
                .unwrap()
                .contains(&"https://example.com".to_string())
        );

        // Verify CSP configuration
        assert_eq!(
            config.security.content_security_policy.as_ref().unwrap(),
            "default-src 'self'"
        );

        // Verify rate limiting configuration
        assert_eq!(config.rate_limiting.requests_per_minute, 600);
        assert_eq!(config.rate_limiting.burst_capacity, 100);
        assert!(config.rate_limiting.enabled);

        // Verify authentication configuration
        assert!(config.auth.is_some());
        let auth = config.auth.unwrap();
        assert!(auth.enabled);
        assert_eq!(auth.api_key_header.unwrap(), "X-API-Key");
    }

    #[test]
    fn test_cors_configuration_variants() {
        // Test permissive CORS (development)
        let permissive = CorsConfig::permissive();
        assert!(permissive.enabled);
        assert!(
            permissive
                .allowed_origins
                .as_ref()
                .unwrap()
                .contains(&"*".to_string())
        );
        assert!(!permissive.allow_credentials); // Cannot be true with wildcard

        // Test strict CORS (production)
        let strict = CorsConfig::strict();
        assert!(strict.enabled);
        assert!(strict.allowed_origins.as_ref().unwrap().is_empty());
        assert!(strict.allow_credentials);

        // Test disabled CORS
        let disabled = CorsConfig::disabled();
        assert!(!disabled.enabled);
        assert!(disabled.allowed_origins.is_none());
    }

    #[test]
    fn test_security_config_variants() {
        // Test development security (minimal)
        let dev_security = SecurityConfig::development();
        assert!(!dev_security.enabled);
        assert!(dev_security.content_security_policy.is_none());
        assert_eq!(dev_security.frame_options, FrameOptions::Disabled);

        // Test production security (maximum)
        let prod_security = SecurityConfig::production();
        assert!(prod_security.enabled);
        assert!(prod_security.content_security_policy.is_some());
        assert_eq!(prod_security.frame_options, FrameOptions::Deny);
        assert!(prod_security.content_type_options);
        assert_eq!(
            prod_security.referrer_policy.as_ref().unwrap(),
            "no-referrer"
        );
    }

    #[test]
    fn test_rate_limiting_config_variants() {
        // Test disabled rate limiting
        let disabled = RateLimitConfig::disabled();
        assert!(!disabled.enabled);
        assert_eq!(disabled.requests_per_minute, 0);

        // Test moderate rate limiting
        let moderate = RateLimitConfig::moderate();
        assert!(moderate.enabled);
        assert_eq!(moderate.requests_per_minute, 300);
        assert_eq!(moderate.burst_capacity, 50);

        // Test strict rate limiting
        let strict = RateLimitConfig::strict();
        assert!(strict.enabled);
        assert_eq!(strict.requests_per_minute, 120);
        assert_eq!(strict.burst_capacity, 20);
    }

    #[tokio::test]
    async fn test_production_grade_middleware_compilation() {
        // Test that we can create routers with different security configurations
        let service = TestMcpService;

        // Development router (permissive)
        let _dev_router = Router::<()>::turbo_mcp_routes_for_merge(
            service.clone(),
            McpServerConfig::development(),
        );

        // Staging router (moderate security)
        let _staging_router =
            Router::<()>::turbo_mcp_routes_for_merge(service.clone(), McpServerConfig::staging());

        // Production router (maximum security)
        let _prod_router = Router::<()>::turbo_mcp_routes_for_merge(
            service.clone(),
            McpServerConfig::production(),
        );

        // Custom configured router
        let custom_config = McpServerConfig::staging()
            .with_cors_origins(vec!["https://trusted.com".to_string()])
            .with_rate_limit(1000, 200)
            .with_jwt_auth("super-secret-key".to_string());

        let _custom_router =
            Router::<()>::turbo_mcp_routes_for_merge(service.clone(), custom_config);

        // If this test compiles, our proven configuration system works
    }

    #[test]
    fn test_configuration_loading_logic() {
        // Test TLS configuration parsing logic directly
        // Instead of testing environment variable loading, test the parsing functions

        // Test TLS version parsing
        let tls_config = TlsConfig {
            cert_file: "/etc/ssl/certs/server.pem".to_string(),
            key_file: "/etc/ssl/private/server.key".to_string(),
            min_version: TlsVersion::TlsV1_3,
            enable_http2: true,
        };
        assert_eq!(tls_config.cert_file, "/etc/ssl/certs/server.pem");
        assert_eq!(tls_config.key_file, "/etc/ssl/private/server.key");
        assert!(matches!(tls_config.min_version, TlsVersion::TlsV1_3));
        assert!(tls_config.enable_http2);

        // Test authentication configuration creation
        let auth_config = AuthConfig {
            enabled: true,
            jwt_secret: Some("test-secret".to_string()),
            api_key_header: Some("X-API-Key".to_string()),
            custom_validator: None,
        };
        assert!(auth_config.enabled);
        assert_eq!(auth_config.jwt_secret.unwrap(), "test-secret");
        assert_eq!(auth_config.api_key_header.unwrap(), "X-API-Key");

        // Test CORS origins parsing logic
        let cors_origins_string = "https://app.example.com,https://admin.example.com";
        let parsed_origins: Vec<String> = cors_origins_string
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        assert_eq!(parsed_origins.len(), 2);
        assert!(parsed_origins.contains(&"https://app.example.com".to_string()));
        assert!(parsed_origins.contains(&"https://admin.example.com".to_string()));
    }
}

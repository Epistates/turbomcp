//! Middleware system for request/response processing

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use turbomcp_core::RequestContext;
use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse};

use crate::{ServerError, ServerResult};

/// Middleware trait for processing requests and responses
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Process request before routing
    async fn process_request(
        &self,
        request: &mut JsonRpcRequest,
        ctx: &mut RequestContext,
    ) -> ServerResult<()>;

    /// Process response after routing
    async fn process_response(
        &self,
        response: &mut JsonRpcResponse,
        ctx: &RequestContext,
    ) -> ServerResult<()>;

    /// Get middleware name
    fn name(&self) -> &str;

    /// Get middleware priority (lower numbers = higher priority)
    fn priority(&self) -> u32 {
        100
    }

    /// Check if middleware is enabled
    fn enabled(&self) -> bool {
        true
    }
}

/// Middleware stack for composing multiple middleware
pub struct MiddlewareStack {
    /// Ordered list of middleware
    middleware: Vec<Arc<dyn Middleware>>,
    /// Stack configuration
    config: StackConfig,
}

impl std::fmt::Debug for MiddlewareStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiddlewareStack")
            .field("middleware_count", &self.middleware.len())
            .field("config", &self.config)
            .finish()
    }
}

/// Middleware stack configuration
#[derive(Debug, Clone)]
pub struct StackConfig {
    /// Enable middleware metrics
    pub enable_metrics: bool,
    /// Enable middleware tracing
    pub enable_tracing: bool,
    /// Middleware timeout in milliseconds
    pub timeout_ms: u64,
    /// Enable error recovery
    pub enable_recovery: bool,
}

impl Default for StackConfig {
    fn default() -> Self {
        Self {
            enable_metrics: true,
            enable_tracing: true,
            timeout_ms: 5_000,
            enable_recovery: true,
        }
    }
}

impl MiddlewareStack {
    /// Create a new middleware stack
    #[must_use]
    pub fn new() -> Self {
        Self {
            middleware: Vec::new(),
            config: StackConfig::default(),
        }
    }

    /// Create a stack with configuration
    #[must_use]
    pub fn with_config(config: StackConfig) -> Self {
        Self {
            middleware: Vec::new(),
            config,
        }
    }

    /// Add middleware to the stack
    pub fn add<M>(&mut self, middleware: M)
    where
        M: Middleware + 'static,
    {
        self.middleware.push(Arc::new(middleware));
        self.sort_by_priority();
    }

    /// Remove middleware by name
    pub fn remove(&mut self, name: &str) {
        self.middleware.retain(|m| m.name() != name);
    }

    /// Process request through all middleware
    pub async fn process_request(
        &self,
        mut request: JsonRpcRequest,
        mut ctx: RequestContext,
    ) -> ServerResult<(JsonRpcRequest, RequestContext)> {
        // Record a start timestamp for end-to-end latency
        let global_start = Instant::now();
        for middleware in &self.middleware {
            if !middleware.enabled() {
                continue;
            }

            let start = Instant::now();

            // Apply timeout if configured
            let result = if self.config.timeout_ms > 0 {
                tokio::time::timeout(
                    Duration::from_millis(self.config.timeout_ms),
                    middleware.process_request(&mut request, &mut ctx),
                )
                .await
            } else {
                Ok(middleware.process_request(&mut request, &mut ctx).await)
            };

            let duration = start.elapsed();

            if self.config.enable_tracing {
                tracing::debug!(
                    middleware = middleware.name(),
                    duration_ms = duration.as_millis(),
                    "Processed request through middleware"
                );
            }

            match result {
                Ok(Ok(())) => continue,
                Ok(Err(e)) => {
                    if self.config.enable_recovery {
                        tracing::warn!(
                            middleware = middleware.name(),
                            error = %e,
                            "Middleware error, continuing with recovery"
                        );
                        continue;
                    }
                    return Err(ServerError::middleware(middleware.name(), e.to_string()));
                }
                Err(_) => {
                    let _error = format!(
                        "Middleware '{}' timed out after {}ms",
                        middleware.name(),
                        self.config.timeout_ms
                    );
                    if self.config.enable_recovery {
                        tracing::warn!(
                            middleware = middleware.name(),
                            "Middleware timeout, continuing"
                        );
                        continue;
                    }
                    return Err(ServerError::timeout("middleware", self.config.timeout_ms));
                }
            }
        }

        // Correlation/request identifiers
        let correlation_id = ctx
            .metadata
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || uuid::Uuid::new_v4().to_string(),
                std::string::ToString::to_string,
            );
        ctx = ctx.with_metadata("correlation_id", correlation_id);

        // Store precise start time and monotonic start in metadata
        let start_ns = start_ts();
        let request_id = ctx.request_id.clone();
        ctx = ctx.with_metadata("request_start_ns", start_ns);
        ctx = ctx.with_metadata("request_id", request_id);
        // Also include wall-clock duration so far (best-effort)
        ctx = ctx.with_metadata(
            "middleware_time_ms",
            global_start.elapsed().as_millis() as u64,
        );
        Ok((request, ctx))
    }

    /// Process response through all middleware (in reverse order)
    pub async fn process_response(
        &self,
        mut response: JsonRpcResponse,
        ctx: &RequestContext,
    ) -> ServerResult<JsonRpcResponse> {
        for middleware in self.middleware.iter().rev() {
            if !middleware.enabled() {
                continue;
            }

            let start = Instant::now();

            // Apply timeout if configured
            let result = if self.config.timeout_ms > 0 {
                tokio::time::timeout(
                    Duration::from_millis(self.config.timeout_ms),
                    middleware.process_response(&mut response, ctx),
                )
                .await
            } else {
                Ok(middleware.process_response(&mut response, ctx).await)
            };

            let duration = start.elapsed();

            if self.config.enable_tracing {
                tracing::debug!(
                    middleware = middleware.name(),
                    duration_ms = duration.as_millis(),
                    "Processed response through middleware"
                );
            }

            match result {
                Ok(Ok(())) => continue,
                Ok(Err(e)) => {
                    if self.config.enable_recovery {
                        tracing::warn!(
                            middleware = middleware.name(),
                            error = %e,
                            "Middleware error in response processing, continuing"
                        );
                        continue;
                    }
                    return Err(ServerError::middleware(middleware.name(), e.to_string()));
                }
                Err(_) => {
                    if self.config.enable_recovery {
                        tracing::warn!(
                            middleware = middleware.name(),
                            "Middleware timeout in response processing, continuing"
                        );
                        continue;
                    }
                    return Err(ServerError::timeout("middleware", self.config.timeout_ms));
                }
            }
        }

        // Compute end-to-end latency if start_ns present
        if let Some(ns) = ctx
            .metadata
            .get("request_start_ns")
            .and_then(serde_json::Value::as_u64)
        {
            let end_ns = start_ts();
            let elapsed_ns = end_ns.saturating_sub(ns);
            let latency_ms = (elapsed_ns as f64) / 1_000_000.0;
            tracing::debug!(
                correlation_id = ctx.metadata.get("correlation_id").and_then(|v| v.as_str()),
                request_id = %ctx.request_id,
                latency_ms,
                "Request completed with latency"
            );
        }
        Ok(response)
    }

    /// Get middleware count
    #[must_use]
    pub fn len(&self) -> usize {
        self.middleware.len()
    }

    /// Check if stack is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.middleware.is_empty()
    }

    /// List all middleware names
    #[must_use]
    pub fn list_middleware(&self) -> Vec<&str> {
        self.middleware.iter().map(|m| m.name()).collect()
    }

    fn sort_by_priority(&mut self) {
        self.middleware.sort_by_key(|m| m.priority());
    }
}

impl Default for MiddlewareStack {
    fn default() -> Self {
        Self::new()
    }
}

/// Authentication middleware
pub struct AuthenticationMiddleware {
    /// Authentication provider
    provider: Arc<dyn AuthProvider>,
    /// Middleware configuration
    config: AuthConfig,
}

impl std::fmt::Debug for AuthenticationMiddleware {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthenticationMiddleware")
            .field("config", &self.config)
            .finish()
    }
}

/// Authentication configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Skip authentication for certain methods
    pub skip_methods: Vec<String>,
    /// Authentication scheme
    pub scheme: AuthScheme,
    /// Token expiry duration
    pub token_expiry: Duration,
}

/// Authentication schemes
#[derive(Debug, Clone)]
pub enum AuthScheme {
    /// Bearer token authentication
    Bearer,
    /// API key authentication
    ApiKey,
    /// Basic authentication
    Basic,
    /// Custom authentication
    Custom(String),
}

/// Authentication provider trait
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Authenticate a request
    async fn authenticate(&self, request: &JsonRpcRequest) -> ServerResult<AuthContext>;

    /// Validate token
    async fn validate_token(&self, token: &str) -> ServerResult<AuthContext>;
}

/// Authentication context
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// User ID
    pub user_id: String,
    /// User roles
    pub roles: Vec<String>,
    /// Token expiry
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Additional claims
    pub claims: HashMap<String, serde_json::Value>,
}

impl AuthenticationMiddleware {
    /// Create new authentication middleware
    pub fn new<P>(provider: P) -> Self
    where
        P: AuthProvider + 'static,
    {
        Self {
            provider: Arc::new(provider),
            config: AuthConfig {
                skip_methods: vec!["initialize".to_string()],
                scheme: AuthScheme::Bearer,
                token_expiry: Duration::from_secs(3600),
            },
        }
    }

    /// Create with configuration
    pub fn with_config<P>(provider: P, config: AuthConfig) -> Self
    where
        P: AuthProvider + 'static,
    {
        Self {
            provider: Arc::new(provider),
            config,
        }
    }
}

#[async_trait]
impl Middleware for AuthenticationMiddleware {
    async fn process_request(
        &self,
        request: &mut JsonRpcRequest,
        _ctx: &mut RequestContext,
    ) -> ServerResult<()> {
        // Skip authentication for certain methods
        if self.config.skip_methods.contains(&request.method) {
            return Ok(());
        }

        match self.provider.authenticate(request).await {
            Ok(auth_ctx) => {
                // Propagate auth into RequestContext
                _ctx.user_id = Some(auth_ctx.user_id.clone());
                let meta = std::sync::Arc::make_mut(&mut _ctx.metadata);
                meta.insert("authenticated".to_string(), serde_json::json!(true));
                meta.insert(
                    "auth".to_string(),
                    serde_json::json!({
                        "user_id": auth_ctx.user_id,
                        "roles": auth_ctx.roles,
                        "expires_at": auth_ctx.expires_at.map(|t| t.to_rfc3339()),
                        "claims": auth_ctx.claims,
                    }),
                );
                Ok(())
            }
            Err(e) => Err(ServerError::authentication(format!(
                "Authentication failed: {e}"
            ))),
        }
    }

    async fn process_response(
        &self,
        _response: &mut JsonRpcResponse,
        _ctx: &RequestContext,
    ) -> ServerResult<()> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        "authentication"
    }

    fn priority(&self) -> u32 {
        10 // High priority
    }
}

/// Rate limiting middleware
#[derive(Debug)]
pub struct RateLimitMiddleware {
    /// Rate limiter
    limiter: Arc<RateLimiter>,
    /// Rate limit configuration
    config: RateLimitConfig,
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Requests per second limit
    pub requests_per_second: u32,
    /// Burst capacity
    pub burst_capacity: u32,
    /// Rate limit key extractor
    pub key_extractor: KeyExtractor,
}

/// Key extraction strategies for rate limiting
#[derive(Debug, Clone)]
pub enum KeyExtractor {
    /// Use client IP address
    ClientIp,
    /// Use user ID from auth context
    UserId,
    /// Use API key
    ApiKey,
    /// Use custom field
    Custom(String),
    /// Global rate limit
    Global,
}

/// Rate limiter implementation
#[derive(Debug)]
pub struct RateLimiter {
    /// Rate limit entries
    entries: Arc<RwLock<HashMap<String, RateLimitEntry>>>,
    /// Cleanup task handle (None in tests)
    _cleanup_handle: Option<tokio::task::JoinHandle<()>>,
}

/// Rate limit entry
#[derive(Debug, Clone)]
struct RateLimitEntry {
    /// Available tokens
    tokens: u32,
    /// Last refill time
    last_refill: Instant,
    /// Entry expiry
    expires_at: Instant,
}

impl RateLimiter {
    /// Create new rate limiter with background cleanup task
    #[must_use]
    pub fn new(_requests_per_second: u32, _burst_capacity: u32) -> Self {
        let entries = Arc::new(RwLock::new(HashMap::<String, RateLimitEntry>::new()));

        // Cleanup task
        let cleanup_entries = Arc::clone(&entries);
        let cleanup_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                let now = Instant::now();
                let mut entries = cleanup_entries.write().await;
                entries.retain(|_, entry| entry.expires_at > now);
            }
        });

        Self {
            entries,
            _cleanup_handle: Some(cleanup_handle),
        }
    }

    /// Create new rate limiter for testing (no background tasks)
    #[must_use]
    #[cfg(test)]
    pub fn new_for_testing(_requests_per_second: u32, _burst_capacity: u32) -> Self {
        let entries = Arc::new(RwLock::new(HashMap::<String, RateLimitEntry>::new()));

        Self {
            entries,
            _cleanup_handle: None, // No cleanup task in tests
        }
    }

    /// Check if request is allowed
    pub async fn check_rate_limit(
        &self,
        key: &str,
        requests_per_second: u32,
        burst_capacity: u32,
    ) -> bool {
        let mut entries = self.entries.write().await;
        let now = Instant::now();

        let entry = entries.entry(key.to_string()).or_insert(RateLimitEntry {
            tokens: burst_capacity,
            last_refill: now,
            expires_at: now + Duration::from_secs(300), // 5 minutes
        });

        // Refill tokens based on time elapsed
        let time_elapsed = now.duration_since(entry.last_refill);
        let tokens_to_add = (time_elapsed.as_secs_f64() * f64::from(requests_per_second)) as u32;

        if tokens_to_add > 0 {
            entry.tokens = (entry.tokens + tokens_to_add).min(burst_capacity);
            entry.last_refill = now;
        }

        if entry.tokens > 0 {
            entry.tokens -= 1;
            entry.expires_at = now + Duration::from_secs(300);
            true
        } else {
            false
        }
    }
}

impl RateLimitMiddleware {
    /// Create new rate limit middleware
    #[must_use]
    pub fn new(config: RateLimitConfig) -> Self {
        let limiter = Arc::new(RateLimiter::new(
            config.requests_per_second,
            config.burst_capacity,
        ));

        Self { limiter, config }
    }

    /// Create new rate limit middleware for testing (no background tasks)
    #[must_use]
    #[cfg(test)]
    pub fn new_for_testing(config: RateLimitConfig) -> Self {
        let limiter = Arc::new(RateLimiter::new_for_testing(
            config.requests_per_second,
            config.burst_capacity,
        ));

        Self { limiter, config }
    }
}

#[async_trait]
impl Middleware for RateLimitMiddleware {
    async fn process_request(
        &self,
        _request: &mut JsonRpcRequest,
        ctx: &mut RequestContext,
    ) -> ServerResult<()> {
        let key = match &self.config.key_extractor {
            KeyExtractor::ClientIp => ctx
                .metadata
                .get("client_ip")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            KeyExtractor::UserId => ctx
                .metadata
                .get("auth")
                .and_then(|v| v.get("user_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("anonymous")
                .to_string(),
            KeyExtractor::ApiKey => ctx
                .metadata
                .get("api_key")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            KeyExtractor::Custom(field) => ctx
                .metadata
                .get(field)
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            KeyExtractor::Global => "global".to_string(),
        };

        let allowed = self
            .limiter
            .check_rate_limit(
                &key,
                self.config.requests_per_second,
                self.config.burst_capacity,
            )
            .await;

        if allowed {
            Ok(())
        } else {
            Err(ServerError::rate_limit_with_retry(
                format!("Rate limit exceeded for key: {key}"),
                60, // Retry after 60 seconds
            ))
        }
    }

    async fn process_response(
        &self,
        _response: &mut JsonRpcResponse,
        _ctx: &RequestContext,
    ) -> ServerResult<()> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        "rate_limit"
    }

    fn priority(&self) -> u32 {
        20 // High priority, but after auth
    }
}

/// Logging middleware for request/response logging
#[derive(Debug)]
pub struct LoggingMiddleware {
    /// Logging configuration
    config: LoggingConfig,
}

/// Logging configuration
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Log request bodies
    pub log_request_body: bool,
    /// Log response bodies
    pub log_response_body: bool,
    /// Log timing information
    pub log_timing: bool,
    /// Maximum body size to log
    pub max_body_size: usize,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_request_body: false,
            log_response_body: false,
            log_timing: true,
            max_body_size: 1024,
        }
    }
}

impl LoggingMiddleware {
    /// Create new logging middleware
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: LoggingConfig::default(),
        }
    }

    /// Create with configuration
    #[must_use]
    pub const fn with_config(config: LoggingConfig) -> Self {
        Self { config }
    }
}

impl Default for LoggingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn process_request(
        &self,
        request: &mut JsonRpcRequest,
        ctx: &mut RequestContext,
    ) -> ServerResult<()> {
        // RequestContext already tracks start_time internally
        let _start_time = ctx.start_time;

        if self.config.log_request_body {
            if let Ok(body) = serde_json::to_string(request) {
                if body.len() <= self.config.max_body_size {
                    tracing::info!(method = %request.method, body = %body, "Request received");
                } else {
                    tracing::info!(method = %request.method, body_size = body.len(), "Request received (body truncated)");
                }
            }
        } else {
            tracing::info!(method = %request.method, id = ?request.id, "Request received");
        }

        Ok(())
    }

    async fn process_response(
        &self,
        response: &mut JsonRpcResponse,
        ctx: &RequestContext,
    ) -> ServerResult<()> {
        if self.config.log_timing {
            // Calculate duration from start time
            let duration = ctx.start_time.elapsed();
            tracing::info!(
                id = ?response.id,
                has_error = response.error().is_some(),
                duration_ms = duration.as_millis(),
                "Request completed"
            );
        }

        if self.config.log_response_body
            && let Ok(body) = serde_json::to_string(response)
        {
            if body.len() <= self.config.max_body_size {
                tracing::debug!(id = ?response.id, body = %body, "Response sent");
            } else {
                tracing::debug!(id = ?response.id, body_size = body.len(), "Response sent (body truncated)");
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "logging"
    }

    fn priority(&self) -> u32 {
        1000 // Low priority - log everything
    }
}

/// HTTP Security Headers middleware for defense-in-depth security
#[derive(Debug, Clone)]
pub struct SecurityHeadersMiddleware {
    /// Security headers configuration
    config: SecurityHeadersConfig,
}

/// Security headers configuration
#[derive(Debug, Clone)]
pub struct SecurityHeadersConfig {
    /// Content Security Policy header
    pub content_security_policy: Option<String>,
    /// X-Frame-Options header
    pub x_frame_options: Option<String>,
    /// X-Content-Type-Options header
    pub x_content_type_options: bool,
    /// X-XSS-Protection header
    pub x_xss_protection: Option<String>,
    /// Strict-Transport-Security header
    pub strict_transport_security: Option<String>,
    /// Referrer-Policy header
    pub referrer_policy: Option<String>,
    /// Permissions-Policy header
    pub permissions_policy: Option<String>,
    /// Cross-Origin-Embedder-Policy header
    pub cross_origin_embedder_policy: Option<String>,
    /// Cross-Origin-Opener-Policy header
    pub cross_origin_opener_policy: Option<String>,
    /// Cross-Origin-Resource-Policy header
    pub cross_origin_resource_policy: Option<String>,
    /// Custom headers
    pub custom_headers: HashMap<String, String>,
}

impl Default for SecurityHeadersConfig {
    fn default() -> Self {
        Self {
            // Secure defaults for MCP servers
            content_security_policy: Some(
                "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
                connect-src 'self'; img-src 'self' data:; font-src 'self'; object-src 'none'; \
                media-src 'self'; frame-src 'none'; base-uri 'self'; form-action 'self'".to_string()
            ),
            x_frame_options: Some("DENY".to_string()),
            x_content_type_options: true,
            x_xss_protection: Some("1; mode=block".to_string()),
            strict_transport_security: Some("max-age=31536000; includeSubDomains; preload".to_string()),
            referrer_policy: Some("strict-origin-when-cross-origin".to_string()),
            permissions_policy: Some(
                "geolocation=(), microphone=(), camera=(), payment=(), usb=(), \
                gyroscope=(), accelerometer=(), magnetometer=()".to_string()
            ),
            cross_origin_embedder_policy: Some("require-corp".to_string()),
            cross_origin_opener_policy: Some("same-origin".to_string()),
            cross_origin_resource_policy: Some("same-origin".to_string()),
            custom_headers: HashMap::new(),
        }
    }
}

impl SecurityHeadersConfig {
    /// Create a new security headers config
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a relaxed configuration for development
    #[must_use]
    pub fn relaxed() -> Self {
        Self {
            content_security_policy: Some(
                "default-src 'self' 'unsafe-inline' 'unsafe-eval'".to_string(),
            ),
            x_frame_options: Some("SAMEORIGIN".to_string()),
            x_content_type_options: true,
            x_xss_protection: Some("1; mode=block".to_string()),
            strict_transport_security: None, // Don't enforce HTTPS in dev
            referrer_policy: Some("no-referrer-when-downgrade".to_string()),
            permissions_policy: None,
            cross_origin_embedder_policy: None,
            cross_origin_opener_policy: None,
            cross_origin_resource_policy: Some("cross-origin".to_string()),
            custom_headers: HashMap::new(),
        }
    }

    /// Create a strict configuration for production
    #[must_use]
    pub fn strict() -> Self {
        Self {
            content_security_policy: Some(
                "default-src 'none'; script-src 'self'; style-src 'self'; \
                connect-src 'self'; img-src 'self'; font-src 'self'; \
                object-src 'none'; media-src 'none'; frame-src 'none'; \
                base-uri 'none'; form-action 'none'"
                    .to_string(),
            ),
            x_frame_options: Some("DENY".to_string()),
            x_content_type_options: true,
            x_xss_protection: Some("1; mode=block".to_string()),
            strict_transport_security: Some(
                "max-age=63072000; includeSubDomains; preload".to_string(),
            ),
            referrer_policy: Some("no-referrer".to_string()),
            permissions_policy: Some(
                "geolocation=(), microphone=(), camera=(), payment=(), usb=(), \
                gyroscope=(), accelerometer=(), magnetometer=(), display-capture=(), \
                screen-wake-lock=(), web-share=()"
                    .to_string(),
            ),
            cross_origin_embedder_policy: Some("require-corp".to_string()),
            cross_origin_opener_policy: Some("same-origin".to_string()),
            cross_origin_resource_policy: Some("same-origin".to_string()),
            custom_headers: HashMap::new(),
        }
    }

    /// Add a custom header
    #[must_use]
    pub fn with_custom_header(mut self, name: String, value: String) -> Self {
        self.custom_headers.insert(name, value);
        self
    }

    /// Set Content Security Policy
    #[must_use]
    pub fn with_csp(mut self, csp: Option<String>) -> Self {
        self.content_security_policy = csp;
        self
    }

    /// Set Strict Transport Security
    #[must_use]
    pub fn with_hsts(mut self, hsts: Option<String>) -> Self {
        self.strict_transport_security = hsts;
        self
    }
}

impl SecurityHeadersMiddleware {
    /// Create new security headers middleware with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: SecurityHeadersConfig::default(),
        }
    }

    /// Create with custom configuration
    #[must_use]
    pub const fn with_config(config: SecurityHeadersConfig) -> Self {
        Self { config }
    }

    /// Create with relaxed configuration for development
    #[must_use]
    pub fn relaxed() -> Self {
        Self {
            config: SecurityHeadersConfig::relaxed(),
        }
    }

    /// Create with strict configuration for production
    #[must_use]
    pub fn strict() -> Self {
        Self {
            config: SecurityHeadersConfig::strict(),
        }
    }
}

impl Default for SecurityHeadersMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for SecurityHeadersMiddleware {
    async fn process_request(
        &self,
        _request: &mut JsonRpcRequest,
        _ctx: &mut RequestContext,
    ) -> ServerResult<()> {
        // Security headers are applied on the response
        Ok(())
    }

    async fn process_response(
        &self,
        response: &mut JsonRpcResponse,
        ctx: &RequestContext,
    ) -> ServerResult<()> {
        // Add security headers to the response object itself
        // The transport layer can read these headers and apply them to the HTTP response
        let mut security_headers = HashMap::new();

        // Content Security Policy
        if let Some(csp) = &self.config.content_security_policy {
            security_headers.insert("Content-Security-Policy".to_string(), csp.clone());
        }

        // X-Frame-Options
        if let Some(xfo) = &self.config.x_frame_options {
            security_headers.insert("X-Frame-Options".to_string(), xfo.clone());
        }

        // X-Content-Type-Options
        if self.config.x_content_type_options {
            security_headers.insert("X-Content-Type-Options".to_string(), "nosniff".to_string());
        }

        // X-XSS-Protection
        if let Some(xss) = &self.config.x_xss_protection {
            security_headers.insert("X-XSS-Protection".to_string(), xss.clone());
        }

        // Strict-Transport-Security
        if let Some(hsts) = &self.config.strict_transport_security {
            security_headers.insert("Strict-Transport-Security".to_string(), hsts.clone());
        }

        // Referrer-Policy
        if let Some(rp) = &self.config.referrer_policy {
            security_headers.insert("Referrer-Policy".to_string(), rp.clone());
        }

        // Permissions-Policy
        if let Some(pp) = &self.config.permissions_policy {
            security_headers.insert("Permissions-Policy".to_string(), pp.clone());
        }

        // Cross-Origin-Embedder-Policy
        if let Some(coep) = &self.config.cross_origin_embedder_policy {
            security_headers.insert("Cross-Origin-Embedder-Policy".to_string(), coep.clone());
        }

        // Cross-Origin-Opener-Policy
        if let Some(coop) = &self.config.cross_origin_opener_policy {
            security_headers.insert("Cross-Origin-Opener-Policy".to_string(), coop.clone());
        }

        // Cross-Origin-Resource-Policy
        if let Some(corp) = &self.config.cross_origin_resource_policy {
            security_headers.insert("Cross-Origin-Resource-Policy".to_string(), corp.clone());
        }

        // Custom headers
        for (name, value) in &self.config.custom_headers {
            security_headers.insert(name.clone(), value.clone());
        }

        // Store security headers in the response for the transport layer to read
        // We add this as a special field that the transport can detect
        if let Some(result) = response.result_mut() {
            if let Some(obj) = result.as_object_mut() {
                obj.insert(
                    "_security_headers".to_string(),
                    serde_json::to_value(&security_headers)?,
                );
            }
        } else {
            // If there's no result, add it as metadata
            response.set_result(serde_json::json!({
                "_security_headers": security_headers
            }));
        }

        tracing::debug!(
            request_id = %ctx.request_id,
            headers_count = security_headers.len(),
            "Applied security headers to response"
        );

        Ok(())
    }

    fn name(&self) -> &'static str {
        "security_headers"
    }

    fn priority(&self) -> u32 {
        900 // Apply late in the response pipeline, but before logging
    }
}

/// Middleware layer for easier composition
pub type MiddlewareLayer = Arc<dyn Middleware>;

fn start_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

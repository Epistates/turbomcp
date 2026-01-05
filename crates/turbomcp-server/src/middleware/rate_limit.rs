//! Rate limiting middleware using tower-governor
//!
//! This middleware implements sophisticated rate limiting using the Generic Cell Rate Algorithm (GCRA)
//! through the tower-governor crate. It supports both global and per-client rate limiting.
//!
//! ## Security (Sprint 3.2)
//!
//! - Per-IP rate limiting with X-Forwarded-For support
//! - Rate limit headers (X-RateLimit-Limit, X-RateLimit-Remaining, X-RateLimit-Reset)
//! - Retry-After header when rate limited
//! - Latest versions: governor 0.10.1 + tower-governor 0.8.0
//!
//! ## Usage
//!
//! ### Zero-Configuration (Recommended)
//!
//! Use `into_governor_layer()` for MCP-compliant rate limiting with automatic
//! JSON-RPC 2.0 error responses:
//!
//! ```rust,ignore
//! use turbomcp_server::middleware::{RateLimitConfig, RateLimitLayer};
//! use axum::Router;
//!
//! let rate_limiter = RateLimitLayer::new(RateLimitConfig::default())
//!     .into_governor_layer();
//!
//! let app = Router::new()
//!     .route("/mcp", post(mcp_handler))
//!     .layer(rate_limiter);
//!
//! // IMPORTANT: Use connect_info for IP extraction
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
//! axum::serve(
//!     listener,
//!     app.into_make_service_with_connect_info::<std::net::SocketAddr>()
//! ).await?;
//! ```
//!
//! ### Manual Configuration
//!
//! For custom error handling or non-HTTP transports, use the helper methods
//! to build your own `GovernorLayer`:
//!
//! ```rust,ignore
//! use turbomcp_server::middleware::{RateLimitConfig, RateLimitLayer};
//! use tower_governor::{GovernorConfigBuilder, GovernorLayer, key_extractor::SmartIpKeyExtractor};
//! use std::sync::Arc;
//!
//! let layer = RateLimitLayer::new(RateLimitConfig::strict());
//!
//! let governor_conf = Arc::new(
//!     GovernorConfigBuilder::default()
//!         .per_second(layer.requests_per_second())
//!         .burst_size(layer.burst_size())
//!         .key_extractor(SmartIpKeyExtractor)
//!         .use_headers()
//!         .finish()
//!         .unwrap()
//! );
//!
//! let rate_limiter = GovernorLayer::new(governor_conf)
//!     .error_handler(my_custom_error_handler);
//! ```

use std::num::NonZeroU32;
use std::time::Duration;

#[cfg(feature = "rate-limiting")]
use std::sync::Arc;

#[cfg(feature = "rate-limiting")]
use bytes::Bytes;
#[cfg(feature = "rate-limiting")]
use governor::middleware::StateInformationMiddleware;
#[cfg(feature = "rate-limiting")]
use http::{Response, StatusCode, header::CONTENT_TYPE};
#[cfg(feature = "rate-limiting")]
use tower_governor::{
    GovernorError, GovernorLayer, governor::GovernorConfigBuilder,
    key_extractor::SmartIpKeyExtractor,
};
#[cfg(feature = "rate-limiting")]
use turbomcp_protocol::{
    error_codes,
    jsonrpc::{JsonRpcError, JsonRpcResponse, JsonRpcResponsePayload, JsonRpcVersion, ResponseId},
};

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Rate limiting strategy
    pub strategy: RateLimitStrategy,
    /// Rate limiting parameters
    pub limits: RateLimits,
    /// Whether to enable rate limiting
    pub enabled: bool,
}

/// Rate limiting strategy
#[derive(Debug, Clone)]
pub enum RateLimitStrategy {
    /// Rate limit by client IP address
    PerIp,
    /// Global rate limiting
    Global,
    /// Custom key extractor (for advanced use cases)
    Custom,
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimits {
    /// Requests per period
    pub requests_per_period: NonZeroU32,
    /// Period duration
    pub period: Duration,
    /// Burst capacity (optional)
    pub burst_size: Option<NonZeroU32>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            strategy: RateLimitStrategy::PerIp,
            limits: RateLimits {
                requests_per_period: NonZeroU32::new(100).unwrap(), // 100 requests
                period: Duration::from_secs(60),                    // per minute
                burst_size: Some(NonZeroU32::new(10).unwrap()),     // allow 10 burst
            },
            enabled: true,
        }
    }
}

impl RateLimitConfig {
    /// Create new rate limit config
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            strategy: RateLimitStrategy::PerIp,
            limits: RateLimits {
                requests_per_period: NonZeroU32::new(requests_per_minute)
                    .unwrap_or(NonZeroU32::new(100).unwrap()),
                period: Duration::from_secs(60),
                burst_size: Some(
                    NonZeroU32::new(requests_per_minute / 10)
                        .unwrap_or(NonZeroU32::new(10).unwrap()),
                ),
            },
            enabled: true,
        }
    }

    /// Set rate limiting strategy
    pub fn with_strategy(mut self, strategy: RateLimitStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set custom rate limits
    pub fn with_limits(mut self, limits: RateLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Enable or disable rate limiting
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Create a strict configuration for high-security environments
    pub fn strict() -> Self {
        Self {
            strategy: RateLimitStrategy::PerIp,
            limits: RateLimits {
                requests_per_period: NonZeroU32::new(30).unwrap(), // 30 requests
                period: Duration::from_secs(60),                   // per minute
                burst_size: Some(NonZeroU32::new(5).unwrap()),     // allow 5 burst
            },
            enabled: true,
        }
    }

    /// Create a permissive configuration for development
    pub fn permissive() -> Self {
        Self {
            strategy: RateLimitStrategy::Global,
            limits: RateLimits {
                requests_per_period: NonZeroU32::new(1000).unwrap(), // 1000 requests
                period: Duration::from_secs(60),                     // per minute
                burst_size: Some(NonZeroU32::new(100).unwrap()),     // allow 100 burst
            },
            enabled: true,
        }
    }
}

/// Rate limiting layer builder
#[derive(Debug, Clone)]
pub struct RateLimitLayer {
    config: RateLimitConfig,
}

impl RateLimitLayer {
    /// Create new rate limiting layer
    pub fn new(config: RateLimitConfig) -> Self {
        Self { config }
    }

    /// Check if rate limiting is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the configuration
    pub fn get_config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Calculate the requests per second rate from the config
    pub fn requests_per_second(&self) -> u64 {
        std::cmp::max(
            1,
            self.config.limits.requests_per_period.get() as u64
                / self.config.limits.period.as_secs(),
        )
    }

    /// Get the burst size from config or calculate from rate
    pub fn burst_size(&self) -> u32 {
        self.config
            .limits
            .burst_size
            .map(|b| b.get())
            .unwrap_or(self.requests_per_second() as u32)
    }

    /// Get the rate limiting configuration ready for tower-governor integration (Sprint 3.2)
    ///
    /// This returns the configuration parameters needed to build a GovernorLayer manually.
    /// Due to tower-governor 0.8.0's complex generic types, users should construct
    /// the layer directly using these parameters.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use turbomcp_server::middleware::RateLimitConfig;
    /// use tower_governor::{GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor};
    /// use std::sync::Arc;
    ///
    /// let config = RateLimitConfig::new(100); // 100 requests per minute
    /// let layer_config = config.get_config();
    ///
    /// // Build the governor config directly
    /// let governor_conf = Arc::new(
    ///     GovernorConfigBuilder::default()
    ///         .per_second(layer_config.requests_per_second.get())
    ///         .burst_size(layer_config.burst_size.get())
    ///         .key_extractor(SmartIpKeyExtractor)
    ///         .use_headers()
    ///         .finish()
    ///         .unwrap()
    /// );
    ///
    /// // Create the layer
    /// let rate_limit_layer = tower_governor::GovernorLayer::new(governor_conf);
    ///
    /// // Use with Axum:
    /// let app = Router::new()
    ///     .route("/api/tools", get(list_tools))
    ///     .layer(rate_limit_layer);
    ///
    /// // CRITICAL: Use this server setup for IP extraction
    /// let server = axum::Server::bind(&addr)
    ///     .serve(app.into_make_service_with_connect_info::<SocketAddr>());
    /// ```
    ///
    /// # Best Practices (from Sprint 3.2 implementation plan)
    ///
    /// 1. ⚠️ **Server Config**: MUST use `.into_make_service_with_connect_info::<SocketAddr>()`
    /// 2. ✅ **Headers**: Use `.use_headers()` for X-RateLimit-* headers
    /// 3. ✅ **Smart IP**: SmartIpKeyExtractor handles X-Forwarded-For, X-Real-IP, CF-Connecting-IP
    /// 4. ✅ **GCRA Algorithm**: Uses Generic Cell Rate Algorithm (most efficient)
    pub fn requests_per_second_nonzero(&self) -> NonZeroU32 {
        NonZeroU32::new(self.requests_per_second() as u32).unwrap_or(NonZeroU32::new(1).unwrap())
    }

    /// Get burst size as NonZeroU32 for governor
    pub fn burst_size_nonzero(&self) -> NonZeroU32 {
        NonZeroU32::new(self.burst_size()).unwrap_or(NonZeroU32::new(1).unwrap())
    }

    /// Create a ready-to-use GovernorLayer with MCP-compliant error responses
    ///
    /// This method creates a fully configured `tower_governor::GovernorLayer` that:
    /// - Uses Smart IP extraction (X-Forwarded-For, X-Real-IP, CF-Connecting-IP, peer IP)
    /// - Returns proper JSON-RPC 2.0 error responses on rate limit (HTTP 429)
    /// - Includes standard rate limit headers (X-RateLimit-*, Retry-After)
    ///
    /// # Returns
    ///
    /// A `GovernorLayer` that can be added to any Tower/Axum service stack.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use turbomcp_server::middleware::{RateLimitConfig, RateLimitLayer};
    /// use axum::Router;
    ///
    /// let app = Router::new()
    ///     .route("/mcp", post(handler))
    ///     .layer(RateLimitLayer::new(RateLimitConfig::default()).into_governor_layer());
    ///
    /// // CRITICAL: Must use connect_info for IP extraction
    /// axum::serve(
    ///     listener,
    ///     app.into_make_service_with_connect_info::<std::net::SocketAddr>()
    /// ).await?;
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the governor configuration is invalid (should never happen with
    /// valid `RateLimitConfig` values).
    #[cfg(feature = "rate-limiting")]
    pub fn into_governor_layer(
        self,
    ) -> GovernorLayer<SmartIpKeyExtractor, StateInformationMiddleware, Bytes> {
        let governor_conf = Arc::new(
            GovernorConfigBuilder::default()
                .per_second(self.requests_per_second())
                .burst_size(self.burst_size())
                .key_extractor(SmartIpKeyExtractor)
                .use_headers()
                .finish()
                .expect("valid governor config from RateLimitConfig"),
        );

        GovernorLayer::new(governor_conf).error_handler(mcp_rate_limit_error_handler)
    }

    /// Create a GovernorLayer with a custom error handler
    ///
    /// Use this when you need custom error response formatting or want to
    /// add additional logging/metrics on rate limit events.
    ///
    /// # Arguments
    ///
    /// * `error_handler` - A function that converts `GovernorError` to `Response<Bytes>`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let layer = RateLimitLayer::new(RateLimitConfig::strict())
    ///     .into_governor_layer_with_handler(|err| {
    ///         tracing::warn!("Rate limit exceeded: {:?}", err);
    ///         // Return custom response
    ///         Response::builder()
    ///             .status(429)
    ///             .body(Bytes::from_static(b"Too many requests"))
    ///             .unwrap()
    ///     });
    /// ```
    #[cfg(feature = "rate-limiting")]
    pub fn into_governor_layer_with_handler<F>(
        self,
        error_handler: F,
    ) -> GovernorLayer<SmartIpKeyExtractor, StateInformationMiddleware, Bytes>
    where
        F: Fn(GovernorError) -> Response<Bytes> + Send + Sync + 'static,
    {
        let governor_conf = Arc::new(
            GovernorConfigBuilder::default()
                .per_second(self.requests_per_second())
                .burst_size(self.burst_size())
                .key_extractor(SmartIpKeyExtractor)
                .use_headers()
                .finish()
                .expect("valid governor config from RateLimitConfig"),
        );

        GovernorLayer::new(governor_conf).error_handler(error_handler)
    }
}

/// MCP-compliant rate limit error handler
///
/// Converts `GovernorError` into a proper JSON-RPC 2.0 error response with:
/// - HTTP 429 Too Many Requests status
/// - MCP error code `-32009` (RATE_LIMITED)
/// - Retry-After header with wait time
/// - X-RateLimit-* headers for client visibility
///
/// # Error Response Format
///
/// ```json
/// {
///   "jsonrpc": "2.0",
///   "error": {
///     "code": -32009,
///     "message": "Rate limit exceeded. Retry after 60 seconds.",
///     "data": { "retry_after_secs": 60 }
///   },
///   "id": null
/// }
/// ```
#[cfg(feature = "rate-limiting")]
pub fn mcp_rate_limit_error_handler(err: GovernorError) -> Response<Bytes> {
    match err {
        GovernorError::TooManyRequests { headers, wait_time } => {
            // Extract retry-after from headers or use wait_time directly (already in seconds)
            let retry_after_secs = headers
                .as_ref()
                .and_then(|h| h.get("retry-after"))
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or_else(|| wait_time.max(1));

            let error_response = JsonRpcResponse {
                jsonrpc: JsonRpcVersion,
                payload: JsonRpcResponsePayload::Error {
                    error: JsonRpcError {
                        code: error_codes::RATE_LIMITED,
                        message: format!(
                            "Rate limit exceeded. Retry after {} seconds.",
                            retry_after_secs
                        ),
                        data: Some(serde_json::json!({
                            "retry_after_secs": retry_after_secs,
                            "error_type": "rate_limit_exceeded"
                        })),
                    },
                },
                id: ResponseId::null(),
            };

            let body = serde_json::to_vec(&error_response)
                .map(Bytes::from)
                .unwrap_or_else(|_| {
                    // Fallback: hand-crafted minimal JSON-RPC error
                    Bytes::from_static(
                        br#"{"jsonrpc":"2.0","error":{"code":-32009,"message":"Rate limit exceeded"},"id":null}"#
                    )
                });

            let mut builder = Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header(CONTENT_TYPE, "application/json")
                .header("retry-after", retry_after_secs.to_string());

            // Forward rate limit headers from governor if present
            if let Some(rate_headers) = headers {
                for (key, value) in rate_headers.iter() {
                    if key.as_str().starts_with("x-ratelimit") {
                        builder = builder.header(key, value);
                    }
                }
            }

            builder
                .body(body)
                .unwrap_or_else(|_| Response::new(Bytes::from_static(b"Rate limited")))
        }

        GovernorError::UnableToExtractKey => {
            // This typically means the request doesn't have IP info
            // (e.g., server not using into_make_service_with_connect_info)
            let error_response = JsonRpcResponse {
                jsonrpc: JsonRpcVersion,
                payload: JsonRpcResponsePayload::Error {
                    error: JsonRpcError {
                        code: error_codes::INTERNAL_ERROR,
                        message: "Unable to identify client for rate limiting".to_string(),
                        data: Some(serde_json::json!({
                            "error_type": "key_extraction_failed",
                            "hint": "Ensure server uses into_make_service_with_connect_info::<SocketAddr>()"
                        })),
                    },
                },
                id: ResponseId::null(),
            };

            let body = serde_json::to_vec(&error_response)
                .map(Bytes::from)
                .unwrap_or_else(|_| Bytes::from_static(br#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Internal error"},"id":null}"#));

            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap_or_else(|_| Response::new(Bytes::from_static(b"Internal error")))
        }

        // Handle any future error variants gracefully
        #[allow(unreachable_patterns)]
        _ => {
            let error_response = JsonRpcResponse {
                jsonrpc: JsonRpcVersion,
                payload: JsonRpcResponsePayload::Error {
                    error: JsonRpcError {
                        code: error_codes::INTERNAL_ERROR,
                        message: "Rate limiting error".to_string(),
                        data: None,
                    },
                },
                id: ResponseId::null(),
            };

            let body = serde_json::to_vec(&error_response)
                .map(Bytes::from)
                .unwrap_or_else(|_| Bytes::from_static(br#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Internal error"},"id":null}"#));

            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap_or_else(|_| Response::new(Bytes::from_static(b"Internal error")))
        }
    }
}

// Note: We use SmartIpKeyExtractor from tower-governor which automatically:
// - Extracts IP from X-Forwarded-For header (with validation)
// - Falls back to X-Real-IP, CF-Connecting-IP, and other standard headers
// - Uses peer IP address as final fallback
// - Handles IPv4 and IPv6 addresses correctly
//
// For custom rate limiting (e.g., by user ID after authentication),
// you can create a custom key extractor and use it with GovernorLayer.
// See tower-governor documentation for examples.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rate_limit_config() {
        let config = RateLimitConfig::default();

        assert!(config.enabled);
        assert_eq!(config.limits.requests_per_period.get(), 100);
        assert_eq!(config.limits.period, Duration::from_secs(60));
        assert_eq!(config.limits.burst_size.unwrap().get(), 10);
    }

    #[test]
    fn test_strict_config() {
        let config = RateLimitConfig::strict();

        assert!(config.enabled);
        assert_eq!(config.limits.requests_per_period.get(), 30);
        assert_eq!(config.limits.burst_size.unwrap().get(), 5);
    }

    #[test]
    fn test_permissive_config() {
        let config = RateLimitConfig::permissive();

        assert!(config.enabled);
        assert_eq!(config.limits.requests_per_period.get(), 1000);
        assert_eq!(config.limits.burst_size.unwrap().get(), 100);
    }

    #[test]
    fn test_custom_rate_limits() {
        let config = RateLimitConfig::new(50).with_limits(RateLimits {
            requests_per_period: NonZeroU32::new(200).unwrap(),
            period: Duration::from_secs(30),
            burst_size: Some(NonZeroU32::new(20).unwrap()),
        });

        assert_eq!(config.limits.requests_per_period.get(), 200);
        assert_eq!(config.limits.period, Duration::from_secs(30));
        assert_eq!(config.limits.burst_size.unwrap().get(), 20);
    }

    #[test]
    fn test_rate_limit_layer_helpers() {
        let config = RateLimitConfig::new(60); // 60 requests per minute = 1 per second
        let layer = RateLimitLayer::new(config);

        assert!(layer.is_enabled());
        assert_eq!(layer.requests_per_second(), 1);
        assert_eq!(layer.burst_size(), 6); // 60/10 = 6
    }

    #[test]
    fn test_requests_per_second_nonzero() {
        let config = RateLimitConfig::new(60); // 60 per minute = 1 per second
        let layer = RateLimitLayer::new(config);

        assert_eq!(layer.requests_per_second_nonzero().get(), 1);
    }

    #[test]
    fn test_burst_size_nonzero() {
        let config = RateLimitConfig::new(100);
        let layer = RateLimitLayer::new(config);

        assert_eq!(layer.burst_size_nonzero().get(), 10); // 100/10 = 10
    }

    #[test]
    #[cfg(feature = "rate-limiting")]
    fn test_governor_config_strict() {
        use tower_governor::{governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor};

        let config = RateLimitConfig::strict();
        let layer = RateLimitLayer::new(config);

        // Verify we can build a valid governor config using our helpers
        let governor_conf = GovernorConfigBuilder::default()
            .per_second(layer.requests_per_second())
            .burst_size(layer.burst_size())
            .key_extractor(SmartIpKeyExtractor)
            .use_headers()
            .finish();

        assert!(
            governor_conf.is_some(),
            "Governor config should build successfully"
        );

        // Verify rate limiting parameters
        assert_eq!(layer.requests_per_second(), 1); // 30 per 60 seconds = 0.5, clamped to min 1
        assert_eq!(layer.burst_size(), 5);
    }

    #[test]
    #[cfg(feature = "rate-limiting")]
    fn test_governor_config_permissive() {
        use tower_governor::{governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor};

        let config = RateLimitConfig::permissive();
        let layer = RateLimitLayer::new(config);

        // Verify we can build a valid governor config using our helpers
        let governor_conf = GovernorConfigBuilder::default()
            .per_second(layer.requests_per_second())
            .burst_size(layer.burst_size())
            .key_extractor(SmartIpKeyExtractor)
            .use_headers()
            .finish();

        assert!(
            governor_conf.is_some(),
            "Governor config should build successfully"
        );

        // Verify rate limiting parameters
        assert_eq!(layer.requests_per_second(), 16); // 1000 per 60 seconds = 16 per second
        assert_eq!(layer.burst_size(), 100);
    }

    #[test]
    #[cfg(feature = "rate-limiting")]
    fn test_into_governor_layer_creates_valid_layer() {
        // Verify the zero-config layer creation works
        let config = RateLimitConfig::default();
        let layer = RateLimitLayer::new(config);

        // This should not panic - if it does, the test fails
        let _governor_layer = layer.into_governor_layer();
    }

    #[test]
    #[cfg(feature = "rate-limiting")]
    fn test_into_governor_layer_with_strict_config() {
        let layer = RateLimitLayer::new(RateLimitConfig::strict());
        let _governor_layer = layer.into_governor_layer();
    }

    #[test]
    #[cfg(feature = "rate-limiting")]
    fn test_into_governor_layer_with_permissive_config() {
        let layer = RateLimitLayer::new(RateLimitConfig::permissive());
        let _governor_layer = layer.into_governor_layer();
    }

    #[test]
    #[cfg(feature = "rate-limiting")]
    fn test_into_governor_layer_with_custom_handler() {
        use http::Response;

        let layer = RateLimitLayer::new(RateLimitConfig::default());
        let _governor_layer = layer.into_governor_layer_with_handler(|_err| {
            Response::builder()
                .status(429)
                .body(Bytes::from_static(b"Custom error"))
                .unwrap()
        });
    }

    #[test]
    #[cfg(feature = "rate-limiting")]
    fn test_mcp_error_handler_too_many_requests() {
        use http::HeaderMap;

        // Create a TooManyRequests error
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "30".parse().unwrap());
        headers.insert("x-ratelimit-limit", "100".parse().unwrap());
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());

        let err = GovernorError::TooManyRequests {
            wait_time: 30, // seconds as u64
            headers: Some(headers),
        };

        let response = mcp_rate_limit_error_handler(err);

        // Verify HTTP status
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Verify headers
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(response.headers().get("retry-after").unwrap(), "30");

        // Verify body is valid JSON-RPC error
        let body = response.into_body();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["error"]["code"], error_codes::RATE_LIMITED);
        assert!(json["error"]["message"].as_str().unwrap().contains("30"));
        assert_eq!(json["error"]["data"]["retry_after_secs"], 30);
        assert!(json["id"].is_null());
    }

    #[test]
    #[cfg(feature = "rate-limiting")]
    fn test_mcp_error_handler_unable_to_extract_key() {
        let err = GovernorError::UnableToExtractKey;
        let response = mcp_rate_limit_error_handler(err);

        // Should return 500 Internal Server Error
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // Verify body
        let body = response.into_body();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["error"]["code"], error_codes::INTERNAL_ERROR);
        assert!(
            json["error"]["data"]["hint"]
                .as_str()
                .unwrap()
                .contains("connect_info")
        );
    }

    #[test]
    #[cfg(feature = "rate-limiting")]
    fn test_mcp_error_handler_without_headers() {
        // Create error without headers
        let err = GovernorError::TooManyRequests {
            wait_time: 60, // seconds as u64
            headers: None,
        };

        let response = mcp_rate_limit_error_handler(err);

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Should default to wait_time when no retry-after header
        assert_eq!(response.headers().get("retry-after").unwrap(), "60");
    }
}

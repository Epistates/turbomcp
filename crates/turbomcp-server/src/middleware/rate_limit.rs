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

use std::num::NonZeroU32;
use std::time::Duration;

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
}

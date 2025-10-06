//! Rate limiting middleware using tower-governor
//!
//! This middleware implements sophisticated rate limiting using the Generic Cell Rate Algorithm (GCRA)
//! through the tower-governor crate. It supports both global and per-client rate limiting.

#![cfg(feature = "rate-limiting")]

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
}

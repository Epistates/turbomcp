//! Rate Limiting for Authentication Endpoints
//!
//! This module provides rate limiting capabilities for OAuth and authentication
//! endpoints to prevent brute-force attacks, credential stuffing, and DoS.
//!
//! ## Features
//!
//! - **Token Bucket Algorithm** - Smooth rate limiting with burst support
//! - **Sliding Window** - Accurate rate limiting over time windows
//! - **Multi-Key Support** - Rate limit by IP, user ID, API key, or composite keys
//! - **Configurable Limits** - Per-endpoint and per-action limits
//! - **Audit Integration** - Logs rate limit events to audit trail
//!
//! ## Security Considerations
//!
//! Rate limiting is a critical defense against:
//! - **Brute Force Attacks** - Limiting password/token guessing attempts
//! - **Credential Stuffing** - Slowing automated credential testing
//! - **Denial of Service** - Preventing resource exhaustion
//! - **Enumeration Attacks** - Slowing user/account discovery
//!
//! ## Usage
//!
//! ```rust,no_run
//! use turbomcp_auth::rate_limit::{RateLimiter, RateLimitConfig, RateLimitKey};
//! use std::time::Duration;
//!
//! # async fn example() {
//! // Create a rate limiter with default auth settings
//! let limiter = RateLimiter::for_auth();
//!
//! // Check if a request should be allowed
//! let key = RateLimitKey::ip("192.168.1.1");
//! match limiter.check(&key, "login").await {
//!     Ok(()) => {
//!         // Request allowed, proceed with authentication
//!     }
//!     Err(info) => {
//!         // Rate limited - return 429 Too Many Requests
//!         println!("Retry after {} seconds", info.retry_after.as_secs());
//!     }
//! }
//!
//! // Composite key for more precise limiting
//! let key = RateLimitKey::composite(vec![
//!     ("ip", "192.168.1.1"),
//!     ("endpoint", "/oauth/token"),
//! ]);
//! # }
//! ```
//!
//! ## Configuration
//!
//! ```rust
//! use turbomcp_auth::rate_limit::{RateLimiter, RateLimitConfig, EndpointLimit};
//! use std::time::Duration;
//!
//! let config = RateLimitConfig::builder()
//!     // Global default
//!     .default_limit(100, Duration::from_secs(60))
//!     // Stricter limit for login attempts
//!     .endpoint_limit("login", EndpointLimit {
//!         requests: 5,
//!         window: Duration::from_secs(60),
//!         burst: 2,
//!     })
//!     // Very strict for token endpoint
//!     .endpoint_limit("token", EndpointLimit {
//!         requests: 10,
//!         window: Duration::from_secs(60),
//!         burst: 3,
//!     })
//!     .build();
//!
//! let limiter = RateLimiter::new(config);
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Rate limiter for authentication endpoints
#[derive(Debug, Clone)]
pub struct RateLimiter {
    config: RateLimitConfig,
    state: Arc<RwLock<RateLimitState>>,
}

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Default limit for endpoints without specific config
    pub default_limit: EndpointLimit,
    /// Per-endpoint limits
    pub endpoint_limits: HashMap<String, EndpointLimit>,
    /// Whether to enable the rate limiter
    pub enabled: bool,
    /// Clean up interval for expired entries
    pub cleanup_interval: Duration,
}

/// Limit configuration for a specific endpoint
#[derive(Debug, Clone)]
pub struct EndpointLimit {
    /// Maximum requests in the time window
    pub requests: u32,
    /// Time window duration
    pub window: Duration,
    /// Burst allowance above the limit
    pub burst: u32,
}

/// Key for rate limiting (IP, user ID, API key, etc.)
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RateLimitKey {
    /// Key type (ip, user, api_key, composite)
    pub key_type: String,
    /// Key value
    pub value: String,
}

/// Information about a rate limit violation
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    /// Time until the limit resets
    pub retry_after: Duration,
    /// Current request count in the window
    pub current_count: u32,
    /// Maximum allowed requests
    pub limit: u32,
    /// Time window
    pub window: Duration,
}

/// Internal state for tracking requests
#[derive(Debug, Default)]
struct RateLimitState {
    /// Map of (key, endpoint) -> request tracking
    entries: HashMap<(RateLimitKey, String), RequestTracker>,
    /// Last cleanup time
    last_cleanup: Option<Instant>,
}

/// Tracks requests for a specific key/endpoint combination
#[derive(Debug, Clone)]
struct RequestTracker {
    /// Request timestamps in the current window
    timestamps: Vec<Instant>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(RateLimitState::default())),
        }
    }

    /// Create a rate limiter with sensible defaults for authentication
    ///
    /// Default limits:
    /// - Login: 5 attempts per minute (burst: 2)
    /// - Token: 10 requests per minute (burst: 3)
    /// - Refresh: 20 requests per minute (burst: 5)
    /// - Other: 100 requests per minute (burst: 10)
    pub fn for_auth() -> Self {
        let config = RateLimitConfig::builder()
            .default_limit(100, Duration::from_secs(60))
            .endpoint_limit(
                "login",
                EndpointLimit {
                    requests: 5,
                    window: Duration::from_secs(60),
                    burst: 2,
                },
            )
            .endpoint_limit(
                "token",
                EndpointLimit {
                    requests: 10,
                    window: Duration::from_secs(60),
                    burst: 3,
                },
            )
            .endpoint_limit(
                "refresh",
                EndpointLimit {
                    requests: 20,
                    window: Duration::from_secs(60),
                    burst: 5,
                },
            )
            .endpoint_limit(
                "authorize",
                EndpointLimit {
                    requests: 10,
                    window: Duration::from_secs(60),
                    burst: 3,
                },
            )
            .endpoint_limit(
                "revoke",
                EndpointLimit {
                    requests: 10,
                    window: Duration::from_secs(60),
                    burst: 2,
                },
            )
            .build();

        Self::new(config)
    }

    /// Create a disabled rate limiter (for testing)
    pub fn disabled() -> Self {
        Self::new(RateLimitConfig {
            default_limit: EndpointLimit {
                requests: u32::MAX,
                window: Duration::from_secs(1),
                burst: 0,
            },
            endpoint_limits: HashMap::new(),
            enabled: false,
            cleanup_interval: Duration::from_secs(3600),
        })
    }

    /// Check if a request should be allowed
    ///
    /// Returns `Ok(())` if allowed, `Err(RateLimitInfo)` if rate limited.
    pub async fn check(&self, key: &RateLimitKey, endpoint: &str) -> Result<(), RateLimitInfo> {
        if !self.config.enabled {
            return Ok(());
        }

        let limit = self
            .config
            .endpoint_limits
            .get(endpoint)
            .unwrap_or(&self.config.default_limit);

        let now = Instant::now();
        let mut state = self.state.write().await;

        // Cleanup expired entries periodically
        self.maybe_cleanup(&mut state, now);

        let entry_key = (key.clone(), endpoint.to_string());
        let tracker = state
            .entries
            .entry(entry_key)
            .or_insert_with(|| RequestTracker {
                timestamps: Vec::new(),
            });

        // Remove timestamps outside the window
        let window_start = now - limit.window;
        tracker.timestamps.retain(|&t| t > window_start);

        // Check if over limit
        let current_count = tracker.timestamps.len() as u32;
        let effective_limit = limit.requests + limit.burst;

        if current_count >= effective_limit {
            // Find oldest timestamp to calculate retry_after
            let oldest = tracker.timestamps.first().copied().unwrap_or(now);
            let retry_after = limit.window - (now - oldest);

            // Record rate limit metric
            crate::auth_metrics::record_rate_limited(endpoint, &key.key_type);

            return Err(RateLimitInfo {
                retry_after,
                current_count,
                limit: limit.requests,
                window: limit.window,
            });
        }

        // Allow request and record timestamp
        tracker.timestamps.push(now);
        Ok(())
    }

    /// Record a request without checking limits (for tracking only)
    pub async fn record(&self, key: &RateLimitKey, endpoint: &str) {
        if !self.config.enabled {
            return;
        }

        let now = Instant::now();
        let mut state = self.state.write().await;

        let entry_key = (key.clone(), endpoint.to_string());
        let tracker = state
            .entries
            .entry(entry_key)
            .or_insert_with(|| RequestTracker {
                timestamps: Vec::new(),
            });

        tracker.timestamps.push(now);
    }

    /// Get current usage for a key/endpoint combination
    pub async fn get_usage(&self, key: &RateLimitKey, endpoint: &str) -> Option<(u32, u32)> {
        let limit = self
            .config
            .endpoint_limits
            .get(endpoint)
            .unwrap_or(&self.config.default_limit);

        let now = Instant::now();
        let state = self.state.read().await;

        let entry_key = (key.clone(), endpoint.to_string());
        state.entries.get(&entry_key).map(|tracker| {
            let window_start = now - limit.window;
            let current = tracker
                .timestamps
                .iter()
                .filter(|&&t| t > window_start)
                .count() as u32;
            (current, limit.requests)
        })
    }

    /// Reset limits for a specific key
    pub async fn reset(&self, key: &RateLimitKey) {
        let mut state = self.state.write().await;
        state.entries.retain(|(k, _), _| k != key);
    }

    /// Reset all limits
    pub async fn reset_all(&self) {
        let mut state = self.state.write().await;
        state.entries.clear();
    }

    fn maybe_cleanup(&self, state: &mut RateLimitState, now: Instant) {
        let should_cleanup = state
            .last_cleanup
            .map(|t| now - t > self.config.cleanup_interval)
            .unwrap_or(true);

        if should_cleanup {
            // Get the maximum window duration
            let max_window = self
                .config
                .endpoint_limits
                .values()
                .map(|l| l.window)
                .max()
                .unwrap_or(self.config.default_limit.window);

            // Remove entries with no recent activity
            let cutoff = now - max_window * 2;
            state.entries.retain(|_, tracker| {
                tracker
                    .timestamps
                    .last()
                    .map(|&t| t > cutoff)
                    .unwrap_or(false)
            });

            state.last_cleanup = Some(now);
        }
    }
}

impl RateLimitConfig {
    /// Create a new configuration builder
    pub fn builder() -> RateLimitConfigBuilder {
        RateLimitConfigBuilder::default()
    }
}

/// Builder for rate limit configuration
#[derive(Debug, Default)]
pub struct RateLimitConfigBuilder {
    default_limit: Option<EndpointLimit>,
    endpoint_limits: HashMap<String, EndpointLimit>,
    enabled: bool,
    cleanup_interval: Option<Duration>,
}

impl RateLimitConfigBuilder {
    /// Set the default limit for endpoints without specific config
    pub fn default_limit(mut self, requests: u32, window: Duration) -> Self {
        self.default_limit = Some(EndpointLimit {
            requests,
            window,
            burst: requests / 10, // 10% burst by default
        });
        self.enabled = true;
        self
    }

    /// Add a limit for a specific endpoint
    pub fn endpoint_limit(mut self, endpoint: impl Into<String>, limit: EndpointLimit) -> Self {
        self.endpoint_limits.insert(endpoint.into(), limit);
        self.enabled = true;
        self
    }

    /// Set the cleanup interval for expired entries
    pub fn cleanup_interval(mut self, interval: Duration) -> Self {
        self.cleanup_interval = Some(interval);
        self
    }

    /// Enable or disable the rate limiter
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Build the configuration
    pub fn build(self) -> RateLimitConfig {
        RateLimitConfig {
            default_limit: self.default_limit.unwrap_or(EndpointLimit {
                requests: 100,
                window: Duration::from_secs(60),
                burst: 10,
            }),
            endpoint_limits: self.endpoint_limits,
            enabled: self.enabled,
            cleanup_interval: self.cleanup_interval.unwrap_or(Duration::from_secs(300)),
        }
    }
}

impl RateLimitKey {
    /// Create a key based on IP address
    pub fn ip(ip: impl Into<String>) -> Self {
        Self {
            key_type: "ip".to_string(),
            value: ip.into(),
        }
    }

    /// Create a key based on user ID
    pub fn user(user_id: impl Into<String>) -> Self {
        Self {
            key_type: "user".to_string(),
            value: user_id.into(),
        }
    }

    /// Create a key based on API key (use prefix only for security)
    pub fn api_key_prefix(prefix: impl Into<String>) -> Self {
        Self {
            key_type: "api_key".to_string(),
            value: prefix.into(),
        }
    }

    /// Create a key based on session ID
    pub fn session(session_id: impl Into<String>) -> Self {
        Self {
            key_type: "session".to_string(),
            value: session_id.into(),
        }
    }

    /// Create a composite key from multiple components
    ///
    /// Useful for more precise rate limiting, e.g., IP + endpoint.
    pub fn composite(components: Vec<(&str, &str)>) -> Self {
        let value = components
            .into_iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join("|");

        Self {
            key_type: "composite".to_string(),
            value,
        }
    }
}

impl std::fmt::Display for RateLimitInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Rate limited: {}/{} requests in {:?}, retry after {:?}",
            self.current_count, self.limit, self.window, self.retry_after
        )
    }
}

impl std::error::Error for RateLimitInfo {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new(
            RateLimitConfig::builder()
                .default_limit(5, Duration::from_secs(60))
                .build(),
        );

        let key = RateLimitKey::ip("192.168.1.1");

        // Should allow 5 requests
        for _ in 0..5 {
            assert!(limiter.check(&key, "test").await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new(
            RateLimitConfig::builder()
                .endpoint_limit(
                    "test",
                    EndpointLimit {
                        requests: 2,
                        window: Duration::from_secs(60),
                        burst: 0,
                    },
                )
                .build(),
        );

        let key = RateLimitKey::ip("192.168.1.1");

        // Should allow 2 requests
        assert!(limiter.check(&key, "test").await.is_ok());
        assert!(limiter.check(&key, "test").await.is_ok());

        // Third should be blocked
        let result = limiter.check(&key, "test").await;
        assert!(result.is_err());

        let info = result.unwrap_err();
        assert_eq!(info.current_count, 2);
        assert_eq!(info.limit, 2);
    }

    #[tokio::test]
    async fn test_rate_limiter_allows_burst() {
        let limiter = RateLimiter::new(
            RateLimitConfig::builder()
                .endpoint_limit(
                    "test",
                    EndpointLimit {
                        requests: 2,
                        window: Duration::from_secs(60),
                        burst: 2,
                    },
                )
                .build(),
        );

        let key = RateLimitKey::ip("192.168.1.1");

        // Should allow 4 requests (2 + 2 burst)
        for i in 0..4 {
            assert!(
                limiter.check(&key, "test").await.is_ok(),
                "Request {} should be allowed",
                i
            );
        }

        // Fifth should be blocked
        assert!(limiter.check(&key, "test").await.is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter_disabled() {
        let limiter = RateLimiter::disabled();
        let key = RateLimitKey::ip("192.168.1.1");

        // Should allow unlimited requests
        for _ in 0..1000 {
            assert!(limiter.check(&key, "test").await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_different_keys() {
        let limiter = RateLimiter::new(
            RateLimitConfig::builder()
                .endpoint_limit(
                    "test",
                    EndpointLimit {
                        requests: 1,
                        window: Duration::from_secs(60),
                        burst: 0,
                    },
                )
                .build(),
        );

        let key1 = RateLimitKey::ip("192.168.1.1");
        let key2 = RateLimitKey::ip("192.168.1.2");

        // Both keys should get their own limit
        assert!(limiter.check(&key1, "test").await.is_ok());
        assert!(limiter.check(&key2, "test").await.is_ok());

        // Both should now be limited
        assert!(limiter.check(&key1, "test").await.is_err());
        assert!(limiter.check(&key2, "test").await.is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter_reset() {
        let limiter = RateLimiter::new(
            RateLimitConfig::builder()
                .endpoint_limit(
                    "test",
                    EndpointLimit {
                        requests: 1,
                        window: Duration::from_secs(60),
                        burst: 0,
                    },
                )
                .build(),
        );

        let key = RateLimitKey::ip("192.168.1.1");

        assert!(limiter.check(&key, "test").await.is_ok());
        assert!(limiter.check(&key, "test").await.is_err());

        // Reset should allow requests again
        limiter.reset(&key).await;
        assert!(limiter.check(&key, "test").await.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_for_auth() {
        let limiter = RateLimiter::for_auth();
        let key = RateLimitKey::ip("192.168.1.1");

        // Login should allow 5 + 2 burst = 7 requests
        for i in 0..7 {
            assert!(
                limiter.check(&key, "login").await.is_ok(),
                "Login request {} should be allowed",
                i
            );
        }
        assert!(limiter.check(&key, "login").await.is_err());
    }

    #[test]
    fn test_rate_limit_key_creation() {
        let ip_key = RateLimitKey::ip("10.0.0.1");
        assert_eq!(ip_key.key_type, "ip");
        assert_eq!(ip_key.value, "10.0.0.1");

        let user_key = RateLimitKey::user("user123");
        assert_eq!(user_key.key_type, "user");

        let composite = RateLimitKey::composite(vec![("ip", "10.0.0.1"), ("endpoint", "/login")]);
        assert_eq!(composite.key_type, "composite");
        assert_eq!(composite.value, "ip:10.0.0.1|endpoint:/login");
    }

    #[tokio::test]
    async fn test_get_usage() {
        let limiter = RateLimiter::new(
            RateLimitConfig::builder()
                .default_limit(10, Duration::from_secs(60))
                .build(),
        );

        let key = RateLimitKey::ip("192.168.1.1");

        // Initially no usage
        assert!(limiter.get_usage(&key, "test").await.is_none());

        // After some requests
        limiter.check(&key, "test").await.ok();
        limiter.check(&key, "test").await.ok();
        limiter.check(&key, "test").await.ok();

        let usage = limiter.get_usage(&key, "test").await;
        assert_eq!(usage, Some((3, 10)));
    }
}

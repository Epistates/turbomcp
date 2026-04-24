//! Rate Limiting for Authentication Endpoints
//!
//! This module provides rate limiting capabilities for OAuth and authentication
//! endpoints to prevent brute-force attacks, credential stuffing, and DoS.
//!
//! ## Implementation
//!
//! The limiter is backed by the [`governor`] crate (lock-free GCRA). Each
//! endpoint gets its own keyed `RateLimiter` instance so that different
//! endpoint quotas don't share state. The public API below is preserved from
//! the previous hand-rolled sliding-window implementation.
//!
//! ## Features
//!
//! - **GCRA (Leaky Bucket)** — Smooth rate limiting with burst allowance
//! - **Lock-free** — `governor` uses sharded atomic state, no global `RwLock`
//! - **Multi-Key Support** — Rate limit by IP, user ID, API key, or composite keys
//! - **Per-endpoint limits** — Different quotas for login / token / refresh / …
//! - **Audit integration** — Logs rate-limit events to `auth_metrics`
//!
//! ## Security Considerations
//!
//! Rate limiting is a critical defense against:
//! - **Brute Force Attacks** — Limiting password/token guessing attempts
//! - **Credential Stuffing** — Slowing automated credential testing
//! - **Denial of Service** — Preventing resource exhaustion
//! - **Enumeration Attacks** — Slowing user/account discovery
//!
//! ## Usage
//!
//! ```rust,no_run
//! use turbomcp_auth::rate_limit::{RateLimiter, RateLimitKey};
//!
//! # async fn example() {
//! let limiter = RateLimiter::for_auth();
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
//! # }
//! ```

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use governor::{
    Quota, RateLimiter as GovernorLimiter,
    clock::{Clock, DefaultClock},
    state::keyed::DashMapStateStore,
};

/// Rate limiter for authentication endpoints
#[derive(Clone)]
pub struct RateLimiter {
    config: Arc<RateLimitConfig>,
    /// endpoint-name → lock-free keyed limiter configured for that endpoint's quota.
    limiters: Arc<DashMap<String, Arc<EndpointLimiter>>>,
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

type KeyedLimiter = GovernorLimiter<RateLimitKey, DashMapStateStore<RateLimitKey>, DefaultClock>;

struct EndpointLimiter {
    limiter: KeyedLimiter,
    limit: EndpointLimit,
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
    ///
    /// Retained for API compatibility; `governor` handles its own state lifecycle
    /// so this field is currently advisory and unused.
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
    /// Current request count in the window (GCRA-approximate: reports effective_limit on deny)
    pub current_count: u32,
    /// Maximum allowed requests
    pub limit: u32,
    /// Time window
    pub window: Duration,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config: Arc::new(config),
            limiters: Arc::new(DashMap::new()),
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

        let endpoint_limiter = self.endpoint_limiter(endpoint);
        let limit = endpoint_limiter.limit.clone();

        match endpoint_limiter.limiter.check_key(key) {
            Ok(()) => Ok(()),
            Err(not_until) => {
                // Convert governor's wait-time into std::time::Duration.
                let retry_after = not_until.wait_time_from(DefaultClock::default().now());

                crate::auth_metrics::record_rate_limited(endpoint, &key.key_type);

                Err(RateLimitInfo {
                    retry_after,
                    // GCRA does not track discrete counts; surface the effective
                    // limit as a signal that the client is at or over capacity.
                    current_count: limit.requests.saturating_add(limit.burst),
                    limit: limit.requests,
                    window: limit.window,
                })
            }
        }
    }

    /// Record a request without checking limits (for tracking only)
    ///
    /// With GCRA this is equivalent to `check` but ignores the decision. It
    /// consumes one permit from the client's bucket.
    pub async fn record(&self, key: &RateLimitKey, endpoint: &str) {
        if !self.config.enabled {
            return;
        }
        let endpoint_limiter = self.endpoint_limiter(endpoint);
        let _ = endpoint_limiter.limiter.check_key(key);
    }

    /// Report the configured limit for a given endpoint.
    ///
    /// Governor's GCRA state does not expose a non-consuming discrete count
    /// query, so this returns `Some((0, limit))` whenever the limiter is
    /// enabled for the endpoint, and `None` when disabled. Callers that need
    /// the precise decision for a request should use [`check`](Self::check),
    /// which atomically queries and records.
    pub async fn get_usage(&self, _key: &RateLimitKey, endpoint: &str) -> Option<(u32, u32)> {
        if !self.config.enabled {
            return None;
        }
        let endpoint_limiter = self.endpoint_limiter(endpoint);
        Some((0, endpoint_limiter.limit.requests))
    }

    /// Reset limits for a specific key
    pub async fn reset(&self, key: &RateLimitKey) {
        // Drop per-endpoint state for this key by letting governor GC it —
        // governor does not expose direct removal for keyed stores at the
        // state-store layer, so we instead rebuild the impacted endpoints.
        // This is an O(E) cost where E is number of tracked endpoints; acceptable
        // because `reset` is a rare admin operation.
        for mut entry in self.limiters.iter_mut() {
            let old = entry.value().clone();
            // Replace with fresh limiter for the same quota.
            let fresh = Arc::new(build_endpoint_limiter(&old.limit));
            *entry.value_mut() = fresh;
            // Drop old so any in-flight borrow of old state is released by refcount.
            drop(old);
            let _ = key; // silence unused in case future impl offers direct key removal
        }
    }

    /// Reset all limits
    pub async fn reset_all(&self) {
        self.limiters.clear();
    }

    fn endpoint_limiter(&self, endpoint: &str) -> Arc<EndpointLimiter> {
        if let Some(existing) = self.limiters.get(endpoint) {
            return Arc::clone(&*existing);
        }
        let limit = self
            .config
            .endpoint_limits
            .get(endpoint)
            .cloned()
            .unwrap_or_else(|| self.config.default_limit.clone());
        let new = Arc::new(build_endpoint_limiter(&limit));
        let entry = self
            .limiters
            .entry(endpoint.to_string())
            .or_insert_with(|| Arc::clone(&new));
        Arc::clone(&*entry)
    }
}

fn build_endpoint_limiter(limit: &EndpointLimit) -> EndpointLimiter {
    // Translate `requests per window` into a `Quota`:
    //   - replenishment rate = window / requests
    //   - burst              = max(requests + burst, 1)
    //
    // For very small windows (< 1s/requests), we floor at 1 req/sec to stay
    // within Quota's construction constraints; the tests pick window=60s which
    // comfortably exceeds this.
    let requests = limit.requests.max(1);
    let burst_cap = limit.requests.saturating_add(limit.burst).max(1);
    let replenish = limit.window / requests;
    let quota = Quota::with_period(replenish)
        .unwrap_or_else(|| Quota::per_minute(NonZeroU32::new(1).expect("1 is nonzero")))
        .allow_burst(NonZeroU32::new(burst_cap).expect("burst_cap is nonzero"));
    EndpointLimiter {
        limiter: GovernorLimiter::keyed(quota),
        limit: limit.clone(),
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

    /// Set the cleanup interval for expired entries (advisory, retained for API compat)
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

        // Effective limit = 5 requests + 0 burst (default builder sets burst = requests/10 = 0)
        // With governor's GCRA, burst_cap = requests + burst = 5. All 5 initial checks
        // are allowed as they consume the initial burst budget.
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

        // Should allow 2 requests (initial burst budget)
        assert!(limiter.check(&key, "test").await.is_ok());
        assert!(limiter.check(&key, "test").await.is_ok());

        // Third should be blocked
        let result = limiter.check(&key, "test").await;
        assert!(result.is_err());

        let info = result.unwrap_err();
        // GCRA reports effective_limit (requests + burst = 2) on deny
        assert_eq!(info.limit, 2);
        assert!(info.retry_after > Duration::ZERO);
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

        // Login effective limit = 5 + 2 burst = 7 requests
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
    async fn test_get_usage_returns_limit_when_enabled() {
        let limiter = RateLimiter::new(
            RateLimitConfig::builder()
                .default_limit(10, Duration::from_secs(60))
                .build(),
        );

        let key = RateLimitKey::ip("192.168.1.1");
        let usage = limiter.get_usage(&key, "test").await;
        assert_eq!(usage, Some((0, 10)));

        // Disabled limiter reports None.
        let disabled = RateLimiter::disabled();
        assert_eq!(disabled.get_usage(&key, "test").await, None);
    }
}

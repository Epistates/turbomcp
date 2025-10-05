//! Rate limiting implementation for transport layer
//!
//! This module provides rate limiting functionality to prevent abuse
//! using a sliding window algorithm. It tracks requests per IP address
//! and enforces configurable limits with automatic cleanup of expired entries.

use super::errors::SecurityError;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Rate limiting configuration
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Maximum requests per window
    pub max_requests: usize,
    /// Time window for rate limiting
    pub window: Duration,
    /// Whether rate limiting is enabled
    pub enabled: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window: Duration::from_secs(60),
            enabled: true,
        }
    }
}

impl RateLimitConfig {
    /// Create a new rate limit configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum requests per window
    pub fn set_max_requests(&mut self, max_requests: usize) {
        self.max_requests = max_requests;
    }

    /// Set time window duration
    pub fn set_window(&mut self, window: Duration) {
        self.window = window;
    }

    /// Enable or disable rate limiting
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

/// Rate limiter state
#[derive(Debug)]
struct RateLimiterState {
    requests: HashMap<IpAddr, Vec<Instant>>,
}

/// Rate limiter implementation using sliding window algorithm
#[derive(Debug)]
pub struct RateLimiter {
    config: RateLimitConfig,
    state: Arc<Mutex<RateLimiterState>>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(RateLimiterState {
                requests: HashMap::new(),
            })),
        }
    }

    /// Create rate limiter with default configuration
    pub fn with_defaults() -> Self {
        Self::new(RateLimitConfig::default())
    }

    /// Check if request is within rate limits
    pub fn check_rate_limit(&self, client_ip: IpAddr) -> Result<(), SecurityError> {
        if !self.config.enabled {
            tracing::debug!(
                client_ip = %client_ip,
                "Rate limiting disabled, allowing request"
            );
            return Ok(());
        }

        let mut state = self.state.lock();
        let now = Instant::now();

        let requests = state.requests.entry(client_ip).or_default();

        // Remove old requests outside the window
        let before_cleanup = requests.len();
        requests.retain(|&time| now.duration_since(time) < self.config.window);
        let after_cleanup = requests.len();

        if before_cleanup != after_cleanup {
            tracing::debug!(
                client_ip = %client_ip,
                before = before_cleanup,
                after = after_cleanup,
                removed = before_cleanup - after_cleanup,
                "Cleaned up expired rate limit entries"
            );
        }

        if requests.len() >= self.config.max_requests {
            tracing::warn!(
                client_ip = %client_ip,
                current = requests.len(),
                limit = self.config.max_requests,
                window_secs = self.config.window.as_secs(),
                "Rate limit exceeded"
            );
            return Err(SecurityError::RateLimitExceeded {
                client: client_ip.to_string(),
                current: requests.len(),
                limit: self.config.max_requests,
            });
        }

        requests.push(now);
        tracing::debug!(
            client_ip = %client_ip,
            current = requests.len(),
            limit = self.config.max_requests,
            remaining = self.config.max_requests - requests.len(),
            "Rate limit check passed"
        );
        Ok(())
    }

    /// Get current request count for a client
    pub fn get_request_count(&self, client_ip: IpAddr) -> usize {
        let mut state = self.state.lock();
        let now = Instant::now();

        if let Some(requests) = state.requests.get_mut(&client_ip) {
            // Clean up expired requests
            requests.retain(|&time| now.duration_since(time) < self.config.window);
            requests.len()
        } else {
            0
        }
    }

    /// Get remaining requests for a client
    pub fn get_remaining_requests(&self, client_ip: IpAddr) -> usize {
        let current = self.get_request_count(client_ip);
        self.config.max_requests.saturating_sub(current)
    }

    /// Clear all rate limit data (useful for testing)
    pub fn clear(&self) {
        let mut state = self.state.lock();
        state.requests.clear();
    }

    /// Clean up expired entries for all clients
    pub fn cleanup_expired(&self) -> usize {
        let mut state = self.state.lock();
        let now = Instant::now();
        let mut cleaned_count = 0;

        state.requests.retain(|_, requests| {
            let original_len = requests.len();
            requests.retain(|&time| now.duration_since(time) < self.config.window);

            if requests.is_empty() {
                cleaned_count += original_len;
                false // Remove the entire entry if no requests remain
            } else {
                cleaned_count += original_len - requests.len();
                true
            }
        });

        cleaned_count
    }

    /// Get total number of tracked clients
    pub fn client_count(&self) -> usize {
        let state = self.state.lock();
        state.requests.len()
    }

    /// Get configuration
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }
}

/// Check rate limits for a client IP using a rate limiter
pub fn check_rate_limit(
    rate_limiter: Option<&RateLimiter>,
    client_ip: IpAddr,
) -> Result<(), SecurityError> {
    if let Some(limiter) = rate_limiter {
        limiter.check_rate_limit(client_ip)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_requests, 100);
        assert_eq!(config.window, Duration::from_secs(60));
        assert!(config.enabled);
    }

    #[test]
    fn test_rate_limiter_allows_requests_within_limit() {
        let config = RateLimitConfig {
            max_requests: 2,
            window: Duration::from_secs(60),
            enabled: true,
        };
        let rate_limiter = RateLimiter::new(config);
        let client_ip = "127.0.0.1".parse().unwrap();

        // First two requests should succeed
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());
    }

    #[test]
    fn test_rate_limiter_blocks_requests_over_limit() {
        let config = RateLimitConfig {
            max_requests: 2,
            window: Duration::from_secs(60),
            enabled: true,
        };
        let rate_limiter = RateLimiter::new(config);
        let client_ip = "127.0.0.1".parse().unwrap();

        // First two requests should succeed
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());

        // Third request should fail
        assert!(rate_limiter.check_rate_limit(client_ip).is_err());
    }

    #[test]
    fn test_rate_limiter_disabled() {
        let config = RateLimitConfig {
            max_requests: 1,
            window: Duration::from_secs(60),
            enabled: false,
        };
        let rate_limiter = RateLimiter::new(config);
        let client_ip = "127.0.0.1".parse().unwrap();

        // Should allow unlimited requests when disabled
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());
    }

    #[test]
    fn test_rate_limiter_sliding_window() {
        let config = RateLimitConfig {
            max_requests: 2,
            window: Duration::from_millis(100),
            enabled: true,
        };
        let rate_limiter = RateLimiter::new(config);
        let client_ip = "127.0.0.1".parse().unwrap();

        // Fill the window
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());
        assert!(rate_limiter.check_rate_limit(client_ip).is_err());

        // Wait for window to expire
        sleep(Duration::from_millis(150));

        // Should allow requests again
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());
    }

    #[test]
    fn test_rate_limiter_different_ips() {
        let config = RateLimitConfig {
            max_requests: 1,
            window: Duration::from_secs(60),
            enabled: true,
        };
        let rate_limiter = RateLimiter::new(config);
        let ip1: IpAddr = "127.0.0.1".parse().unwrap();
        let ip2: IpAddr = "192.168.1.1".parse().unwrap();

        // Each IP should have separate limits
        assert!(rate_limiter.check_rate_limit(ip1).is_ok());
        assert!(rate_limiter.check_rate_limit(ip2).is_ok());

        // Both should be at limit now
        assert!(rate_limiter.check_rate_limit(ip1).is_err());
        assert!(rate_limiter.check_rate_limit(ip2).is_err());
    }

    #[test]
    fn test_get_request_count() {
        let config = RateLimitConfig {
            max_requests: 2,
            window: Duration::from_secs(1),
            enabled: true,
        };
        let rate_limiter = RateLimiter::new(config);
        let client_ip = "127.0.0.1".parse().unwrap();

        assert_eq!(rate_limiter.get_request_count(client_ip), 0);

        rate_limiter.check_rate_limit(client_ip).unwrap();
        assert_eq!(rate_limiter.get_request_count(client_ip), 1);

        rate_limiter.check_rate_limit(client_ip).unwrap();
        assert_eq!(rate_limiter.get_request_count(client_ip), 2);
    }

    #[test]
    fn test_get_remaining_requests() {
        let config = RateLimitConfig {
            max_requests: 3,
            window: Duration::from_secs(60),
            enabled: true,
        };
        let rate_limiter = RateLimiter::new(config);
        let client_ip = "127.0.0.1".parse().unwrap();

        assert_eq!(rate_limiter.get_remaining_requests(client_ip), 3);

        rate_limiter.check_rate_limit(client_ip).unwrap();
        assert_eq!(rate_limiter.get_remaining_requests(client_ip), 2);

        rate_limiter.check_rate_limit(client_ip).unwrap();
        assert_eq!(rate_limiter.get_remaining_requests(client_ip), 1);
    }

    #[test]
    fn test_cleanup_expired() {
        let config = RateLimitConfig {
            max_requests: 5,
            window: Duration::from_millis(50),
            enabled: true,
        };
        let rate_limiter = RateLimiter::new(config);
        let client_ip = "127.0.0.1".parse().unwrap();

        // Add some requests
        rate_limiter.check_rate_limit(client_ip).unwrap();
        rate_limiter.check_rate_limit(client_ip).unwrap();

        // Wait for expiration
        sleep(Duration::from_millis(100));

        // Cleanup should remove expired entries
        let cleaned = rate_limiter.cleanup_expired();
        assert!(cleaned > 0);
        assert_eq!(rate_limiter.get_request_count(client_ip), 0);
    }
}

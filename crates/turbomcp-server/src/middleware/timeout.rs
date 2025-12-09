//! Timeout middleware for request timeout management
//!
//! This middleware provides configurable request timeouts to prevent
//! hanging requests and resource exhaustion.

use std::time::Duration;

use http::StatusCode;
use tower_http::timeout::TimeoutLayer as HttpTimeoutLayer;

/// Timeout configuration
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Request timeout duration
    pub request_timeout: Duration,
    /// Whether timeouts are enabled
    pub enabled: bool,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30), // 30 second default
            enabled: true,
        }
    }
}

impl TimeoutConfig {
    /// Create new timeout config
    pub fn new(request_timeout: Duration) -> Self {
        Self {
            request_timeout,
            enabled: true,
        }
    }

    /// Set request timeout
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Enable or disable timeouts
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Create a strict timeout configuration
    pub fn strict() -> Self {
        Self {
            request_timeout: Duration::from_secs(10), // 10 seconds
            enabled: true,
        }
    }

    /// Create a permissive timeout configuration
    pub fn permissive() -> Self {
        Self {
            request_timeout: Duration::from_secs(120), // 2 minutes
            enabled: true,
        }
    }

    /// Disable timeouts (for development)
    pub fn disabled() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            enabled: false,
        }
    }
}

/// Timeout layer
#[derive(Debug, Clone)]
pub struct TimeoutLayer {
    config: TimeoutConfig,
}

impl TimeoutLayer {
    /// Create new timeout layer
    pub fn new(config: TimeoutConfig) -> Self {
        Self { config }
    }

    /// Build the timeout middleware
    /// Returns None if timeouts are disabled
    pub fn build(self) -> Option<HttpTimeoutLayer> {
        if self.config.enabled {
            Some(HttpTimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                self.config.request_timeout,
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_timeout_config() {
        let config = TimeoutConfig::default();

        assert!(config.enabled);
        assert_eq!(config.request_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_strict_config() {
        let config = TimeoutConfig::strict();

        assert!(config.enabled);
        assert_eq!(config.request_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_permissive_config() {
        let config = TimeoutConfig::permissive();

        assert!(config.enabled);
        assert_eq!(config.request_timeout, Duration::from_secs(120));
    }

    #[test]
    fn test_disabled_config() {
        let config = TimeoutConfig::disabled();

        assert!(!config.enabled);
    }

    #[test]
    fn test_custom_timeout() {
        let config = TimeoutConfig::new(Duration::from_secs(45))
            .with_request_timeout(Duration::from_secs(60));

        assert_eq!(config.request_timeout, Duration::from_secs(60));
    }
}

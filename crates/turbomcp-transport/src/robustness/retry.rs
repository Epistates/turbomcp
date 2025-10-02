//! Retry mechanisms with exponential backoff and jitter
//!
//! This module provides sophisticated retry logic for transport operations with:
//! - Exponential backoff with configurable multipliers
//! - Jitter to prevent thundering herd effects
//! - Custom retry conditions based on error patterns
//! - Configurable retry policies for different error types

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Retry configuration for transport operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Base delay between retries
    pub base_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Exponential backoff multiplier
    pub backoff_multiplier: f64,
    /// Jitter factor (0.0 - 1.0) to avoid thundering herd
    pub jitter_factor: f64,
    /// Whether to retry on connection errors
    pub retry_on_connection_error: bool,
    /// Whether to retry on timeout errors
    pub retry_on_timeout: bool,
    /// Custom retry conditions
    pub custom_retry_conditions: Vec<RetryCondition>,
}

/// Custom retry condition based on error patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryCondition {
    /// Error pattern to match
    pub error_pattern: String,
    /// Whether to retry on this condition
    pub should_retry: bool,
    /// Override delay for this condition
    pub custom_delay: Option<Duration>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter_factor: 0.1,
            retry_on_connection_error: true,
            retry_on_timeout: true,
            custom_retry_conditions: Vec::new(),
        }
    }
}

impl RetryConfig {
    /// Create a new retry configuration with sensible defaults for MCP transport
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a retry configuration optimized for network operations
    pub fn for_network() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 1.5,
            jitter_factor: 0.2,
            retry_on_connection_error: true,
            retry_on_timeout: true,
            custom_retry_conditions: Vec::new(),
        }
    }

    /// Create a retry configuration optimized for I/O operations
    pub fn for_io() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter_factor: 0.1,
            retry_on_connection_error: false,
            retry_on_timeout: true,
            custom_retry_conditions: Vec::new(),
        }
    }

    /// Calculate the delay for a given attempt with exponential backoff and jitter
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return self.base_delay;
        }

        // Calculate exponential backoff
        let delay_ms =
            self.base_delay.as_millis() as f64 * self.backoff_multiplier.powi(attempt as i32 - 1);

        // Apply jitter
        let jitter = 1.0 + (fastrand::f64() - 0.5) * 2.0 * self.jitter_factor;
        let jittered_delay_ms = delay_ms * jitter;

        // Cap at max delay
        let capped_delay_ms = jittered_delay_ms.min(self.max_delay.as_millis() as f64);

        Duration::from_millis(capped_delay_ms as u64)
    }

    /// Check if an error should be retried based on the configuration
    pub fn should_retry(&self, error: &str, attempt: u32) -> bool {
        if attempt >= self.max_attempts {
            return false;
        }

        // Check custom retry conditions first
        for condition in &self.custom_retry_conditions {
            if error.contains(&condition.error_pattern) {
                return condition.should_retry;
            }
        }

        // Check built-in retry conditions
        if self.retry_on_connection_error && is_connection_error(error) {
            return true;
        }

        if self.retry_on_timeout && is_timeout_error(error) {
            return true;
        }

        false
    }

    /// Get custom delay for a specific error pattern
    pub fn get_custom_delay(&self, error: &str) -> Option<Duration> {
        for condition in &self.custom_retry_conditions {
            if error.contains(&condition.error_pattern) {
                return condition.custom_delay;
            }
        }
        None
    }
}

/// Check if an error message indicates a connection error
fn is_connection_error(error: &str) -> bool {
    let connection_patterns = [
        "connection refused",
        "connection reset",
        "connection timeout",
        "network unreachable",
        "host unreachable",
        "no route to host",
        "connection aborted",
        "broken pipe",
    ];

    let error_lower = error.to_lowercase();
    connection_patterns
        .iter()
        .any(|pattern| error_lower.contains(pattern))
}

/// Check if an error message indicates a timeout
fn is_timeout_error(error: &str) -> bool {
    let timeout_patterns = [
        "timeout",
        "timed out",
        "deadline exceeded",
        "operation timeout",
    ];

    let error_lower = error.to_lowercase();
    timeout_patterns
        .iter()
        .any(|pattern| error_lower.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.base_delay, Duration::from_millis(100));
        assert_eq!(config.backoff_multiplier, 2.0);
    }

    #[test]
    fn test_calculate_delay_exponential_backoff() {
        let config = RetryConfig::default();

        let delay0 = config.calculate_delay(0);
        assert_eq!(delay0, Duration::from_millis(100));

        // Allow for jitter in testing
        let delay1 = config.calculate_delay(1);
        assert!(delay1.as_millis() >= 90 && delay1.as_millis() <= 220);
    }

    #[test]
    fn test_should_retry_connection_errors() {
        let config = RetryConfig::default();

        assert!(config.should_retry("connection refused", 1));
        assert!(config.should_retry("Connection timeout occurred", 1));
        assert!(!config.should_retry("invalid json", 1));
    }

    #[test]
    fn test_should_retry_max_attempts() {
        let config = RetryConfig::default();

        assert!(!config.should_retry("connection refused", 3));
        assert!(!config.should_retry("connection refused", 4));
    }

    #[test]
    fn test_custom_retry_conditions() {
        let mut config = RetryConfig::default();
        config.custom_retry_conditions.push(RetryCondition {
            error_pattern: "custom error".to_string(),
            should_retry: true,
            custom_delay: Some(Duration::from_millis(500)),
        });

        assert!(config.should_retry("this is a custom error", 1));
        assert_eq!(
            config.get_custom_delay("custom error"),
            Some(Duration::from_millis(500))
        );
    }
}

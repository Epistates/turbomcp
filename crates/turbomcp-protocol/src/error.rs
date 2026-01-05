//! Error handling types for MCP protocol.
//!
//! v3.0: The primary error type is now `McpError` from `turbomcp-core`.
//! This module only contains supplementary types like `RetryInfo`.
//!
//! For error handling, use:
//! - `turbomcp_protocol::McpError` - The unified error type
//! - `turbomcp_protocol::ErrorKind` - Error classification
//! - `turbomcp_protocol::McpResult<T>` - Result alias

use serde::{Deserialize, Serialize};

/// Information about retry attempts
///
/// This type is used to track retry state for operations that may need
/// multiple attempts. It includes the number of attempts made, the
/// maximum allowed, and an optional delay before the next retry.
///
/// # Example
///
/// ```rust
/// use turbomcp_protocol::error::RetryInfo;
///
/// let retry_info = RetryInfo {
///     attempts: 2,
///     max_attempts: 5,
///     retry_after_ms: Some(1000),
/// };
///
/// assert!(!retry_info.exhausted());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryInfo {
    /// Number of attempts made so far
    pub attempts: u32,

    /// Maximum attempts allowed before giving up
    pub max_attempts: u32,

    /// Suggested delay in milliseconds before the next retry attempt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

impl RetryInfo {
    /// Create new retry info with default values
    ///
    /// # Arguments
    ///
    /// * `max_attempts` - Maximum number of retry attempts allowed
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_protocol::error::RetryInfo;
    ///
    /// let retry_info = RetryInfo::new(3);
    /// assert_eq!(retry_info.attempts, 0);
    /// assert_eq!(retry_info.max_attempts, 3);
    /// ```
    #[must_use]
    pub const fn new(max_attempts: u32) -> Self {
        Self {
            attempts: 0,
            max_attempts,
            retry_after_ms: None,
        }
    }

    /// Create retry info with a specific delay
    ///
    /// # Arguments
    ///
    /// * `max_attempts` - Maximum number of retry attempts allowed
    /// * `retry_after_ms` - Delay in milliseconds before next retry
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_protocol::error::RetryInfo;
    ///
    /// let retry_info = RetryInfo::with_delay(3, 1000);
    /// assert_eq!(retry_info.retry_after_ms, Some(1000));
    /// ```
    #[must_use]
    pub const fn with_delay(max_attempts: u32, retry_after_ms: u64) -> Self {
        Self {
            attempts: 0,
            max_attempts,
            retry_after_ms: Some(retry_after_ms),
        }
    }

    /// Check if retry attempts are exhausted
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_protocol::error::RetryInfo;
    ///
    /// let mut retry_info = RetryInfo::new(2);
    /// assert!(!retry_info.exhausted());
    ///
    /// retry_info.attempts = 2;
    /// assert!(retry_info.exhausted());
    /// ```
    #[must_use]
    pub const fn exhausted(&self) -> bool {
        self.attempts >= self.max_attempts
    }

    /// Increment the attempt counter
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_protocol::error::RetryInfo;
    ///
    /// let mut retry_info = RetryInfo::new(3);
    /// retry_info.increment();
    /// assert_eq!(retry_info.attempts, 1);
    /// ```
    pub fn increment(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
    }

    /// Get remaining attempts
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_protocol::error::RetryInfo;
    ///
    /// let mut retry_info = RetryInfo::new(5);
    /// retry_info.attempts = 2;
    /// assert_eq!(retry_info.remaining(), 3);
    /// ```
    #[must_use]
    pub const fn remaining(&self) -> u32 {
        self.max_attempts.saturating_sub(self.attempts)
    }
}

impl Default for RetryInfo {
    fn default() -> Self {
        Self::new(3) // Default to 3 retry attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_info_creation() {
        let retry_info = RetryInfo::new(5);
        assert_eq!(retry_info.attempts, 0);
        assert_eq!(retry_info.max_attempts, 5);
        assert_eq!(retry_info.retry_after_ms, None);
    }

    #[test]
    fn test_retry_info_with_delay() {
        let retry_info = RetryInfo::with_delay(3, 1000);
        assert_eq!(retry_info.attempts, 0);
        assert_eq!(retry_info.max_attempts, 3);
        assert_eq!(retry_info.retry_after_ms, Some(1000));
    }

    #[test]
    fn test_retry_exhausted() {
        let mut retry_info = RetryInfo::new(2);
        assert!(!retry_info.exhausted());

        retry_info.attempts = 1;
        assert!(!retry_info.exhausted());

        retry_info.attempts = 2;
        assert!(retry_info.exhausted());

        retry_info.attempts = 3;
        assert!(retry_info.exhausted());
    }

    #[test]
    fn test_retry_increment() {
        let mut retry_info = RetryInfo::new(5);
        assert_eq!(retry_info.attempts, 0);

        retry_info.increment();
        assert_eq!(retry_info.attempts, 1);

        retry_info.increment();
        assert_eq!(retry_info.attempts, 2);
    }

    #[test]
    fn test_retry_remaining() {
        let mut retry_info = RetryInfo::new(5);
        assert_eq!(retry_info.remaining(), 5);

        retry_info.attempts = 2;
        assert_eq!(retry_info.remaining(), 3);

        retry_info.attempts = 5;
        assert_eq!(retry_info.remaining(), 0);
    }

    #[test]
    fn test_retry_default() {
        let retry_info = RetryInfo::default();
        assert_eq!(retry_info.max_attempts, 3);
        assert_eq!(retry_info.attempts, 0);
    }

    #[test]
    fn test_retry_serialization() {
        let retry_info = RetryInfo {
            attempts: 2,
            max_attempts: 5,
            retry_after_ms: Some(1000),
        };

        let json = serde_json::to_string(&retry_info).unwrap();
        let deserialized: RetryInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(retry_info, deserialized);
    }

    #[test]
    fn test_retry_serialization_no_delay() {
        let retry_info = RetryInfo::new(3);

        let json = serde_json::to_string(&retry_info).unwrap();
        assert!(!json.contains("retry_after_ms")); // Should be skipped when None

        let deserialized: RetryInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(retry_info, deserialized);
    }
}

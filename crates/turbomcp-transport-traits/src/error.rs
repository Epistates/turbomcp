//! Transport error types.

use std::time::Duration;
use thiserror::Error;

use crate::config::LimitsConfig;

/// A specialized `Result` type for transport operations.
pub type TransportResult<T> = std::result::Result<T, TransportError>;

/// Represents errors that can occur during transport operations.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum TransportError {
    /// Failed to establish a connection.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// An established connection was lost.
    #[error("Connection lost: {0}")]
    ConnectionLost(String),

    /// Failed to send a message.
    #[error("Send failed: {0}")]
    SendFailed(String),

    /// Failed to receive a message.
    #[error("Receive failed: {0}")]
    ReceiveFailed(String),

    /// Failed to serialize or deserialize a message.
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    /// A protocol-level error occurred.
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// The operation did not complete within the specified timeout.
    #[error("Operation timed out")]
    Timeout,

    /// Connection establishment timed out.
    #[error(
        "Connection timed out after {timeout:?} for operation: {operation}. \
         If this is expected, increase the timeout with \
         `TimeoutConfig {{ connect: Duration::from_secs({}) }}`",
        timeout.as_secs() * 2
    )]
    ConnectionTimeout {
        /// The operation that timed out
        operation: String,
        /// The timeout duration that was exceeded
        timeout: Duration,
    },

    /// Single request timed out.
    #[error(
        "Request timed out after {timeout:?} for operation: {operation}. \
         If this is expected, increase the timeout with \
         `TimeoutConfig {{ request: Some(Duration::from_secs({})) }}` \
         or use `TimeoutConfig::patient()` for slow operations",
        timeout.as_secs() * 2
    )]
    RequestTimeout {
        /// The operation that timed out
        operation: String,
        /// The timeout duration that was exceeded
        timeout: Duration,
    },

    /// Total operation timed out (including retries).
    #[error(
        "Total operation timed out after {timeout:?} for operation: {operation}. \
         This includes retries. If this is expected, increase the timeout with \
         `TimeoutConfig {{ total: Some(Duration::from_secs({})) }}`",
        timeout.as_secs() * 2
    )]
    TotalTimeout {
        /// The operation that timed out
        operation: String,
        /// The timeout duration that was exceeded
        timeout: Duration,
    },

    /// Read operation timed out (streaming).
    #[error(
        "Read timed out after {timeout:?} while streaming response for operation: {operation}. \
         If this is expected, increase the timeout with \
         `TimeoutConfig {{ read: Some(Duration::from_secs({})) }}`",
        timeout.as_secs() * 2
    )]
    ReadTimeout {
        /// The operation that timed out
        operation: String,
        /// The timeout duration that was exceeded
        timeout: Duration,
    },

    /// The transport was configured with invalid parameters.
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// Authentication with the remote endpoint failed.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// The request was rejected due to rate limiting.
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// The requested transport is not available.
    #[error("Transport not available: {0}")]
    NotAvailable(String),

    /// An underlying I/O error occurred.
    #[error("IO error: {0}")]
    Io(String),

    /// An unexpected internal error occurred.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Request size exceeds the configured maximum limit.
    #[error(
        "Request size ({size} bytes) exceeds maximum allowed ({max} bytes). \
         If this is expected, increase the limit with \
         `LimitsConfig {{ max_request_size: Some({}) }}` or use `LimitsConfig::unlimited()` \
         if running behind an API gateway.",
        size
    )]
    RequestTooLarge {
        /// The actual size of the request in bytes
        size: usize,
        /// The maximum allowed size in bytes
        max: usize,
    },

    /// Response size exceeds the configured maximum limit.
    #[error(
        "Response size ({size} bytes) exceeds maximum allowed ({max} bytes). \
         If this is expected, increase the limit with \
         `LimitsConfig {{ max_response_size: Some({}) }}` or use `LimitsConfig::unlimited()` \
         if running behind an API gateway.",
        size
    )]
    ResponseTooLarge {
        /// The actual size of the response in bytes
        size: usize,
        /// The maximum allowed size in bytes
        max: usize,
    },
}

impl From<std::io::Error> for TransportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

impl From<serde_json::Error> for TransportError {
    fn from(err: serde_json::Error) -> Self {
        Self::SerializationFailed(err.to_string())
    }
}

/// Validates that a request message size does not exceed the configured limit.
///
/// # Arguments
///
/// * `size` - The size of the request payload in bytes
/// * `limits` - The limits configuration to check against
///
/// # Returns
///
/// `Ok(())` if the size is within limits or no limit is set, otherwise `Err(TransportError::RequestTooLarge)`
pub fn validate_request_size(size: usize, limits: &LimitsConfig) -> TransportResult<()> {
    if let Some(max_size) = limits.max_request_size
        && size > max_size
    {
        return Err(TransportError::RequestTooLarge {
            size,
            max: max_size,
        });
    }
    Ok(())
}

/// Validates that a response message size does not exceed the configured limit.
///
/// # Arguments
///
/// * `size` - The size of the response payload in bytes
/// * `limits` - The limits configuration to check against
///
/// # Returns
///
/// `Ok(())` if the size is within limits or no limit is set, otherwise `Err(TransportError::ResponseTooLarge)`
pub fn validate_response_size(size: usize, limits: &LimitsConfig) -> TransportResult<()> {
    if let Some(max_size) = limits.max_response_size
        && size > max_size
    {
        return Err(TransportError::ResponseTooLarge {
            size,
            max: max_size,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_size_validation() {
        let limits = LimitsConfig::default();
        assert!(validate_request_size(1000, &limits).is_ok());
        assert!(validate_request_size(10 * 1024 * 1024, &limits).is_err());
    }

    #[test]
    fn test_response_size_validation() {
        let limits = LimitsConfig::default();
        assert!(validate_response_size(1000, &limits).is_ok());
        assert!(validate_response_size(50 * 1024 * 1024, &limits).is_err());
    }

    #[test]
    fn test_unlimited_config() {
        let limits = LimitsConfig::unlimited();
        assert!(validate_request_size(100 * 1024 * 1024, &limits).is_ok());
        assert!(validate_response_size(100 * 1024 * 1024, &limits).is_ok());
    }
}

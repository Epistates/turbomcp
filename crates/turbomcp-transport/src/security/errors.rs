//! Security-related error types for transport layer
//!
//! This module defines all security-related errors that can occur during
//! transport operations, including origin validation, authentication,
//! rate limiting, session security, and message size validation.

use thiserror::Error;

/// Security-related errors
#[derive(Error, Debug)]
pub enum SecurityError {
    /// Origin header validation failed
    #[error("Origin header validation failed: {0}")]
    InvalidOrigin(String),

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Rate limit exceeded for client
    #[error("Rate limit exceeded for {client}: {current}/{limit} requests")]
    RateLimitExceeded {
        /// Client identifier
        client: String,
        /// Current request count
        current: usize,
        /// Rate limit threshold
        limit: usize,
    },

    /// Session security violation
    #[error("Session security violation: {0}")]
    SessionViolation(String),

    /// Message too large
    #[error("Message too large: {size} bytes exceeds limit of {limit} bytes")]
    MessageTooLarge {
        /// Message size in bytes
        size: usize,
        /// Size limit in bytes
        limit: usize,
    },
}

impl SecurityError {
    /// Convert security error to HTTP status code
    pub fn to_http_status(&self) -> u16 {
        match self {
            SecurityError::InvalidOrigin(_) => 403,         // Forbidden
            SecurityError::AuthenticationFailed(_) => 401,  // Unauthorized
            SecurityError::RateLimitExceeded { .. } => 429, // Too Many Requests
            SecurityError::SessionViolation(_) => 403,      // Forbidden
            SecurityError::MessageTooLarge { .. } => 413,   // Payload Too Large
        }
    }
}

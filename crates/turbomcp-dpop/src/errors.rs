//! DPoP error types and conversions
//!
//! This module provides error handling for DPoP operations with detailed
//! context for cryptographic and protocol-level failures.

use std::fmt;

use thiserror::Error;

/// DPoP-specific errors with detailed context for debugging and security analysis
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum DpopError {
    /// Invalid DPoP proof structure or format
    #[error("Invalid DPoP proof structure: {reason}")]
    InvalidProofStructure {
        /// Detailed reason for the validation failure
        reason: String,
    },

    /// Cryptographic operation failed
    #[error("Cryptographic error: {reason}")]
    CryptographicError {
        /// Detailed reason for the cryptographic failure
        reason: String,
    },

    /// DPoP proof validation failed
    #[error("DPoP proof validation failed: {reason}")]
    ProofValidationFailed {
        /// Detailed reason for the validation failure
        reason: String,
    },

    /// Key management operation failed
    #[error("Key management error: {reason}")]
    KeyManagementError {
        /// Detailed reason for the key operation failure
        reason: String,
    },

    /// DPoP proof has expired
    #[error("DPoP proof expired: issued at {issued_at}, max age {max_age_seconds}s")]
    ProofExpired {
        /// When the proof was issued (Unix timestamp)
        issued_at: i64,
        /// Maximum allowed age in seconds
        max_age_seconds: u64,
    },

    /// Replay attack detected - nonce already used
    #[error("Replay attack detected: nonce '{nonce}' already used")]
    ReplayAttackDetected {
        /// The nonce that was already used
        nonce: String,
    },

    /// Clock skew too large between client and server
    #[error("Clock skew too large: {skew_seconds}s exceeds maximum {max_skew_seconds}s")]
    ClockSkewTooLarge {
        /// Actual clock skew in seconds
        skew_seconds: i64,
        /// Maximum allowed skew in seconds
        max_skew_seconds: i64,
    },

    /// DPoP thumbprint mismatch
    #[error("DPoP thumbprint mismatch: expected '{expected}', got '{actual}'")]
    ThumbprintMismatch {
        /// Expected thumbprint
        expected: String,
        /// Actual thumbprint from proof
        actual: String,
    },

    /// HTTP method/URI binding validation failed
    #[error("HTTP binding validation failed: {reason}")]
    HttpBindingFailed {
        /// Detailed reason for the binding failure
        reason: String,
    },

    /// Access token hash validation failed
    #[error("Access token hash validation failed: {reason}")]
    AccessTokenHashFailed {
        /// Detailed reason for the hash validation failure
        reason: String,
    },

    /// Key storage operation failed
    #[error("Key storage error: {reason}")]
    KeyStorageError {
        /// Detailed reason for the storage failure
        reason: String,
    },

    /// Nonce storage operation failed
    #[error("Storage error: {reason}")]
    StorageError {
        /// Detailed reason for the storage failure
        reason: String,
    },

    /// Configuration error
    #[error("Configuration error: {reason}")]
    ConfigurationError {
        /// Detailed reason for the configuration error
        reason: String,
    },

    /// Network or I/O error during DPoP operations
    #[error("I/O error during DPoP operation: {reason}")]
    IoError {
        /// Detailed reason for the I/O failure
        reason: String,
    },

    /// Serialization/deserialization error
    #[error("Serialization error: {reason}")]
    SerializationError {
        /// Detailed reason for the serialization failure
        reason: String,
    },

    /// Internal error that should not occur in normal operation
    #[error("Internal DPoP error: {reason}")]
    InternalError {
        /// Detailed reason for the internal error
        reason: String,
    },
}

impl DpopError {
    /// Check if this error indicates a security violation
    #[must_use]
    pub fn is_security_violation(&self) -> bool {
        matches!(
            self,
            Self::ReplayAttackDetected { .. }
                | Self::ThumbprintMismatch { .. }
                | Self::ProofValidationFailed { .. }
                | Self::AccessTokenHashFailed { .. }
        )
    }

    /// Check if this error is due to client clock skew
    #[must_use]
    pub fn is_clock_skew_error(&self) -> bool {
        matches!(
            self,
            Self::ClockSkewTooLarge { .. } | Self::ProofExpired { .. }
        )
    }

    /// Check if this error is a cryptographic failure
    #[must_use]
    pub fn is_cryptographic_error(&self) -> bool {
        matches!(
            self,
            Self::CryptographicError { .. } | Self::KeyManagementError { .. }
        )
    }

    /// Get error severity for logging and monitoring
    #[must_use]
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            // Critical security violations
            Self::ReplayAttackDetected { .. } => ErrorSeverity::Critical,
            Self::ThumbprintMismatch { .. } => ErrorSeverity::Critical,

            // High severity errors
            Self::CryptographicError { .. } => ErrorSeverity::High,
            Self::KeyManagementError { .. } => ErrorSeverity::High,
            Self::ProofValidationFailed { .. } => ErrorSeverity::High,
            Self::AccessTokenHashFailed { .. } => ErrorSeverity::High,

            // Medium severity errors (often client-side issues)
            Self::ClockSkewTooLarge { .. } => ErrorSeverity::Medium,
            Self::ProofExpired { .. } => ErrorSeverity::Medium,
            Self::InvalidProofStructure { .. } => ErrorSeverity::Medium,
            Self::HttpBindingFailed { .. } => ErrorSeverity::Medium,

            // Low severity errors (configuration/operational issues)
            Self::ConfigurationError { .. } => ErrorSeverity::Low,
            Self::KeyStorageError { .. } => ErrorSeverity::Low,
            Self::StorageError { .. } => ErrorSeverity::Low,
            Self::IoError { .. } => ErrorSeverity::Low,
            Self::SerializationError { .. } => ErrorSeverity::Low,

            // Internal errors (should not occur)
            Self::InternalError { .. } => ErrorSeverity::Critical,
        }
    }

    /// Get suggested remediation for this error
    #[must_use]
    pub fn remediation_hint(&self) -> &'static str {
        match self {
            Self::ClockSkewTooLarge { .. } => "Synchronize system clock with NTP server",
            Self::ProofExpired { .. } => "Generate a new DPoP proof with current timestamp",
            Self::ReplayAttackDetected { .. } => "Generate a new DPoP proof with unique nonce",
            Self::ThumbprintMismatch { .. } => {
                "Verify DPoP key pair matches the expected thumbprint"
            }
            Self::InvalidProofStructure { .. } => "Check DPoP proof format against RFC 9449",
            Self::CryptographicError { .. } => "Verify cryptographic key material and algorithms",
            Self::KeyManagementError { .. } => "Check key storage and rotation configuration",
            Self::ConfigurationError { .. } => "Review DPoP configuration parameters",
            _ => "Check logs for detailed error information",
        }
    }
}

/// Error severity levels for monitoring and alerting
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorSeverity {
    /// Low severity - operational issues that don't affect security
    Low,
    /// Medium severity - client errors or misconfigurations
    Medium,
    /// High severity - server-side errors affecting functionality
    High,
    /// Critical severity - security violations requiring immediate attention
    Critical,
}

impl fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "LOW"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::High => write!(f, "HIGH"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

// Note: Integration with TurboMCP error hierarchy is done at the application level
// where turbomcp::McpError can convert from DpopError as needed. This crate
// remains focused on DPoP-specific functionality.

// Conversions from common error types
impl From<std::io::Error> for DpopError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError {
            reason: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for DpopError {
    fn from(err: serde_json::Error) -> Self {
        Self::SerializationError {
            reason: err.to_string(),
        }
    }
}

impl From<jsonwebtoken::errors::Error> for DpopError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        use jsonwebtoken::errors::ErrorKind;

        match err.kind() {
            ErrorKind::InvalidToken => Self::InvalidProofStructure {
                reason: "Invalid JWT structure".to_string(),
            },
            ErrorKind::InvalidSignature => Self::CryptographicError {
                reason: "Invalid JWT signature".to_string(),
            },
            ErrorKind::ExpiredSignature => Self::ProofExpired {
                issued_at: 0, // Will be filled in by caller
                max_age_seconds: 0,
            },
            ErrorKind::InvalidIssuer => Self::ProofValidationFailed {
                reason: "Invalid issuer".to_string(),
            },
            ErrorKind::InvalidAudience => Self::ProofValidationFailed {
                reason: "Invalid audience".to_string(),
            },
            _ => Self::CryptographicError {
                reason: format!("JWT error: {err}"),
            },
        }
    }
}

// Error conversion for ring cryptographic library
impl From<ring::error::Unspecified> for DpopError {
    fn from(_: ring::error::Unspecified) -> Self {
        Self::CryptographicError {
            reason: "Ring cryptographic operation failed".to_string(),
        }
    }
}

// Error conversion for r2d2 session pool (only used by PKCS#11)
#[cfg(feature = "hsm-pkcs11")]
impl From<r2d2::Error> for DpopError {
    fn from(err: r2d2::Error) -> Self {
        Self::InternalError {
            reason: format!("Session pool error: {}", err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_severity_classification() {
        let replay_error = DpopError::ReplayAttackDetected {
            nonce: "test-nonce".to_string(),
        };
        assert_eq!(replay_error.severity(), ErrorSeverity::Critical);
        assert!(replay_error.is_security_violation());

        let clock_error = DpopError::ClockSkewTooLarge {
            skew_seconds: 400,
            max_skew_seconds: 300,
        };
        assert_eq!(clock_error.severity(), ErrorSeverity::Medium);
        assert!(clock_error.is_clock_skew_error());

        let crypto_error = DpopError::CryptographicError {
            reason: "Invalid key".to_string(),
        };
        assert_eq!(crypto_error.severity(), ErrorSeverity::High);
        assert!(crypto_error.is_cryptographic_error());
    }

    // Note: Error conversion tests are done at the application level
    // where the main turbomcp crate handles DpopError -> McpError conversion

    #[test]
    fn test_remediation_hints() {
        let clock_error = DpopError::ClockSkewTooLarge {
            skew_seconds: 400,
            max_skew_seconds: 300,
        };
        assert_eq!(
            clock_error.remediation_hint(),
            "Synchronize system clock with NTP server"
        );

        let replay_error = DpopError::ReplayAttackDetected {
            nonce: "test-nonce".to_string(),
        };
        assert_eq!(
            replay_error.remediation_hint(),
            "Generate a new DPoP proof with unique nonce"
        );
    }
}

//! Security error types for TurboMCP file operations

use thiserror::Error;

/// Result type for security operations
pub type SecurityResult<T> = Result<T, SecurityError>;

/// Comprehensive security error types
#[derive(Error, Debug, Clone)]
pub enum SecurityError {
    #[error("Path traversal attack detected: {0}")]
    PathTraversal(String),

    #[error("Symlink attack detected: {0}")]
    SymlinkAttack(String),

    #[error("Path outside allowed boundaries: {0}")]
    UnauthorizedPath(String),

    #[error("File extension not allowed: {0}")]
    ForbiddenExtension(String),

    #[error("File size exceeds limit: {actual} bytes > {limit} bytes")]
    FileSizeLimitExceeded { actual: u64, limit: u64 },

    #[error("Directory depth exceeds limit: {actual} > {limit}")]
    DirectoryDepthExceeded { actual: usize, limit: usize },

    #[error("Resource limit exceeded: {resource_type}: {details}")]
    ResourceLimitExceeded {
        resource_type: String,
        details: String,
    },

    #[error("Concurrent access limit exceeded: {current}/{limit} for {resource}")]
    ConcurrencyLimitExceeded {
        current: usize,
        limit: usize,
        resource: String,
    },

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Audit logging failed: {0}")]
    AuditError(String),

    #[error("Security policy violation: {0}")]
    PolicyViolation(String),
}

impl SecurityError {
    /// Check if this is a critical security error that should trigger alerts
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            SecurityError::PathTraversal(_)
                | SecurityError::SymlinkAttack(_)
                | SecurityError::UnauthorizedPath(_)
        )
    }

    /// Get error category for metrics and logging
    pub fn category(&self) -> &'static str {
        match self {
            SecurityError::PathTraversal(_) => "path_traversal",
            SecurityError::SymlinkAttack(_) => "symlink_attack",
            SecurityError::UnauthorizedPath(_) => "unauthorized_path",
            SecurityError::ForbiddenExtension(_) => "forbidden_extension",
            SecurityError::FileSizeLimitExceeded { .. } => "file_size_limit",
            SecurityError::DirectoryDepthExceeded { .. } => "directory_depth_limit",
            SecurityError::ResourceLimitExceeded { .. } => "resource_limit",
            SecurityError::ConcurrencyLimitExceeded { .. } => "concurrency_limit",
            SecurityError::InvalidInput(_) => "invalid_input",
            SecurityError::PermissionDenied(_) => "permission_denied",
            SecurityError::IoError(_) => "io_error",
            SecurityError::ConfigurationError(_) => "configuration_error",
            SecurityError::AuditError(_) => "audit_error",
            SecurityError::PolicyViolation(_) => "policy_violation",
        }
    }
}

/// Convert standard I/O errors to security errors with context
impl From<std::io::Error> for SecurityError {
    fn from(error: std::io::Error) -> Self {
        SecurityError::IoError(format!("I/O operation failed: {}", error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_categorization() {
        let traversal_error = SecurityError::PathTraversal("../etc/passwd".to_string());
        assert!(traversal_error.is_critical());
        assert_eq!(traversal_error.category(), "path_traversal");

        let size_error = SecurityError::FileSizeLimitExceeded {
            actual: 2000,
            limit: 1000,
        };
        assert!(!size_error.is_critical());
        assert_eq!(size_error.category(), "file_size_limit");
    }

    #[test]
    fn test_error_display() {
        let error = SecurityError::PathTraversal("../sensitive".to_string());
        assert_eq!(
            error.to_string(),
            "Path traversal attack detected: ../sensitive"
        );

        let size_error = SecurityError::FileSizeLimitExceeded {
            actual: 2048,
            limit: 1024,
        };
        assert_eq!(
            size_error.to_string(),
            "File size exceeds limit: 2048 bytes > 1024 bytes"
        );
    }

    #[test]
    fn test_io_error_conversion() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let security_error: SecurityError = io_error.into();

        match security_error {
            SecurityError::IoError(msg) => {
                assert!(msg.contains("File not found"));
            }
            _ => panic!("Expected IoError variant"),
        }
    }
}

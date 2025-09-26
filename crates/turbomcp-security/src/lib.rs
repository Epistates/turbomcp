//! TurboMCP Security Module
//!
//! Production-grade file safety and path validation for TurboMCP.
//! Implements enterprise security patterns to prevent path traversal,
//! symlink attacks, and resource exhaustion.
//!
//! # Security Features
//!
//! - **Path Canonicalization**: Resolves all symbolic links and relative paths
//! - **Traversal Prevention**: Prevents directory traversal attacks (../, symlinks)
//! - **Resource Limits**: File size, depth, and concurrency protection
//! - **Whitelist/Blacklist**: Configurable path restrictions
//! - **Audit Logging**: Security event logging for monitoring
//!
//! # Example
//!
//! ```rust
//! use turbomcp_security::{PathValidator, SecurityPolicy};
//!
//! let policy = SecurityPolicy::default()
//!     .max_file_size(1024 * 1024) // 1MB limit
//!     .allowed_extensions(&[".json", ".txt"])
//!     .forbidden_paths(&["/etc", "/proc"]);
//!
//! let validator = PathValidator::new(policy);
//! let safe_path = validator.validate_path("/tmp/data.json")?;
//! ```

pub mod audit;
pub mod error;
pub mod path;
pub mod policy;
pub mod resource;

pub use audit::{AuditLogger, SecurityEvent};
pub use error::{SecurityError, SecurityResult};
pub use path::{PathValidator, SecurityPolicy};
pub use resource::{ResourceLimiter, ResourcePolicy};

// Cedar policy engine types (available when cedar-policies feature is enabled)
#[cfg(feature = "cedar-policies")]
pub use policy::{
    AccessRequest, PolicyContext, PolicyDecision, PolicyEngine, PolicyStats,
    DEFAULT_FILE_ACCESS_POLICIES,
};

// Basic policy types (available without Cedar feature)
#[cfg(not(feature = "cedar-policies"))]
pub use policy::{
    AccessRequest, PolicyContext, PolicyDecision, PolicyStats, DEFAULT_FILE_ACCESS_POLICIES,
};

use std::path::Path;
use std::sync::Arc;

/// Comprehensive file security validator
#[derive(Debug, Clone)]
pub struct FileSecurityValidator {
    path_validator: PathValidator,
    resource_limiter: Arc<ResourceLimiter>,
    audit_logger: Arc<AuditLogger>,
}

impl FileSecurityValidator {
    /// Create a new file security validator with production-grade defaults
    pub fn new() -> Self {
        let policy = SecurityPolicy::default();
        let resource_policy = ResourcePolicy::default();
        let audit_logger = AuditLogger::new();

        Self {
            path_validator: PathValidator::new(policy),
            resource_limiter: Arc::new(ResourceLimiter::new(resource_policy)),
            audit_logger: Arc::new(audit_logger),
        }
    }

    /// Create with custom policies
    pub fn with_policies(
        security_policy: SecurityPolicy,
        resource_policy: ResourcePolicy,
        audit_logger: AuditLogger,
    ) -> Self {
        Self {
            path_validator: PathValidator::new(security_policy),
            resource_limiter: Arc::new(ResourceLimiter::new(resource_policy)),
            audit_logger: Arc::new(audit_logger),
        }
    }

    /// Validate and sanitize a file path for safe access
    pub async fn validate_file_access(&self, path: &Path) -> SecurityResult<std::path::PathBuf> {
        // Log security event
        self.audit_logger
            .log_event(SecurityEvent::FileAccessAttempt {
                path: path.to_path_buf(),
                timestamp: chrono::Utc::now(),
            })
            .await;

        // Validate path security
        let safe_path = self.path_validator.validate_path(path)?;

        // Check resource limits
        self.resource_limiter.check_file_access(&safe_path).await?;

        // Log successful validation
        self.audit_logger
            .log_event(SecurityEvent::FileAccessGranted {
                path: safe_path.clone(),
                timestamp: chrono::Utc::now(),
            })
            .await;

        Ok(safe_path)
    }

    /// Validate file operations before memory mapping
    pub async fn validate_mmap_access(
        &self,
        path: &Path,
        offset: usize,
        length: Option<usize>,
    ) -> SecurityResult<(std::path::PathBuf, usize, usize)> {
        let safe_path = self.validate_file_access(path).await?;

        // Get file metadata for size validation
        let metadata = tokio::fs::metadata(&safe_path)
            .await
            .map_err(|e| SecurityError::IoError(format!("Failed to read metadata: {}", e)))?;

        let file_size = metadata.len() as usize;

        // Validate offset bounds
        if offset >= file_size {
            return Err(SecurityError::InvalidInput(format!(
                "Offset {} exceeds file size {}",
                offset, file_size
            )));
        }

        // Calculate and validate actual length
        let actual_length = length.unwrap_or(file_size - offset);
        let actual_length = actual_length.min(file_size - offset);

        // Validate against resource limits
        self.resource_limiter
            .check_mmap_access(actual_length)
            .await?;

        self.audit_logger
            .log_event(SecurityEvent::MemoryMapAccess {
                path: safe_path.clone(),
                offset,
                length: actual_length,
                timestamp: chrono::Utc::now(),
            })
            .await;

        Ok((safe_path, offset, actual_length))
    }

    /// Validate socket path for Unix domain sockets
    pub async fn validate_socket_path(&self, path: &Path) -> SecurityResult<std::path::PathBuf> {
        // Special validation for socket files
        let safe_path = self.path_validator.validate_socket_path(path)?;

        // Check directory permissions
        if let Some(parent) = safe_path.parent() {
            self.resource_limiter.check_directory_access(parent).await?;
        }

        self.audit_logger
            .log_event(SecurityEvent::SocketPathValidated {
                path: safe_path.clone(),
                timestamp: chrono::Utc::now(),
            })
            .await;

        Ok(safe_path)
    }
}

impl Default for FileSecurityValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_file_security_validator_basic() {
        // Create validator with symlinks allowed for temp directory on macOS
        let security_policy = SecurityPolicy::default().allow_symlinks(true);
        let resource_policy = ResourcePolicy::default();
        let audit_logger = AuditLogger::new();
        let validator =
            FileSecurityValidator::with_policies(security_policy, resource_policy, audit_logger);
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.json");

        // Create test file
        tokio::fs::write(&test_file, b"test content").await.unwrap();

        // Should validate successfully
        let result = validator.validate_file_access(&test_file).await;
        assert!(result.is_ok());

        let safe_path = result.unwrap();
        assert_eq!(
            safe_path.canonicalize().unwrap(),
            test_file.canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn test_path_traversal_prevention() {
        let validator = FileSecurityValidator::new();

        // These should all be rejected
        let malicious_paths = [
            "../../../etc/passwd",
            "/proc/self/mem",
            "../../sensitive/data",
        ];

        for malicious_path in &malicious_paths {
            let result = validator
                .validate_file_access(Path::new(malicious_path))
                .await;
            assert!(
                result.is_err(),
                "Should reject malicious path: {}",
                malicious_path
            );
        }
    }

    #[tokio::test]
    async fn test_mmap_validation() {
        // Create validator with symlinks allowed for temp directory on macOS
        let security_policy = SecurityPolicy::default().allow_symlinks(true);
        let resource_policy = ResourcePolicy::default();
        let audit_logger = AuditLogger::new();
        let validator =
            FileSecurityValidator::with_policies(security_policy, resource_policy, audit_logger);
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("mmap_test.bin");

        // Create test file with known content
        let test_data = vec![0u8; 1024];
        tokio::fs::write(&test_file, &test_data).await.unwrap();

        // Validate mmap access
        let result = validator
            .validate_mmap_access(&test_file, 0, Some(512))
            .await;
        assert!(result.is_ok());

        let (safe_path, offset, length) = result.unwrap();
        assert_eq!(offset, 0);
        assert_eq!(length, 512);
        assert_eq!(
            safe_path.canonicalize().unwrap(),
            test_file.canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn test_socket_path_validation() {
        let validator = FileSecurityValidator::new();
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let result = validator.validate_socket_path(&socket_path).await;
        assert!(result.is_ok());
    }
}

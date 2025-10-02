//! Simple, robust path validation for security
//!
//! This module provides focused path validation utilities to prevent common
//! security vulnerabilities like path traversal attacks. It follows the principle
//! of doing one thing well rather than trying to cover every possible security scenario.

use crate::Result;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Validates a path for basic security constraints
///
/// This function performs essential security checks:
/// - Canonicalizes the path to resolve symlinks and relative components
/// - Prevents path traversal attacks by checking for ".." patterns
/// - Validates that the path is within reasonable bounds
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_core::security::validate_path;
///
/// // Safe path
/// let safe_path = validate_path("/home/user/data.txt")?;
///
/// // Path traversal attempt - will fail
/// let result = validate_path("/home/user/../../../etc/passwd");
/// assert!(result.is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn validate_path<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    let path = path.as_ref();
    debug!("Validating path: {:?}", path);

    // Check for obvious path traversal patterns before filesystem operations
    let path_str = path.to_string_lossy();
    if path_str.contains("..") {
        return Err(crate::Error::security(format!(
            "Path traversal pattern detected: {:?}",
            path
        )));
    }

    // Canonicalize the path to resolve symlinks and relative components
    let canonical_path = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to canonicalize path {:?}: {}", path, e);
            return Err(crate::Error::security(format!(
                "Invalid path or access denied: {:?}",
                path
            )));
        }
    };

    // Basic sanity check on path depth to prevent excessive nesting
    let depth = canonical_path.components().count();
    if depth > 20 {
        // Reasonable limit for most use cases
        return Err(crate::Error::security(format!(
            "Path depth too deep ({}): {:?}",
            depth, canonical_path
        )));
    }

    debug!("Path validation successful: {:?}", canonical_path);
    Ok(canonical_path)
}

/// Validates a path and enforces it's within a base directory
///
/// This is useful for ensuring file operations stay within allowed boundaries.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_core::security::validate_path_within;
///
/// let base = "/home/user/workspace";
/// let file_path = validate_path_within("/home/user/workspace/project/file.txt", base)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn validate_path_within<P: AsRef<Path>, B: AsRef<Path>>(path: P, base: B) -> Result<PathBuf> {
    let validated_path = validate_path(path)?;
    let base_path = base
        .as_ref()
        .canonicalize()
        .map_err(|e| crate::Error::security(format!("Invalid base path: {}", e)))?;

    if !validated_path.starts_with(&base_path) {
        return Err(crate::Error::security(format!(
            "Path outside allowed directory: {:?} not within {:?}",
            validated_path, base_path
        )));
    }

    Ok(validated_path)
}

/// Checks if a file extension is allowed
///
/// Simple utility for validating file extensions against an allow list.
pub fn validate_file_extension<P: AsRef<Path>>(path: P, allowed_extensions: &[&str]) -> Result<()> {
    let path = path.as_ref();

    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => {
            if allowed_extensions.contains(&ext) {
                Ok(())
            } else {
                Err(crate::Error::security(format!(
                    "File extension '{}' not allowed",
                    ext
                )))
            }
        }
        None => {
            if allowed_extensions.is_empty() {
                Ok(()) // No extension required
            } else {
                Err(crate::Error::security(
                    "File must have an extension".to_string(),
                ))
            }
        }
    }
}

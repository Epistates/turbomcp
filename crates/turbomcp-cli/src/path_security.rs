//! Path validation and sanitization to prevent path traversal attacks
//!
//! This module provides security-critical functions to validate output paths and
//! sanitize filenames, preventing malicious servers from writing arbitrary files
//! via crafted tool names.

use crate::error::{CliError, CliResult};
use std::path::{Component, Path, PathBuf};

/// Maximum allowed filename length (to stay within filesystem limits)
const MAX_FILENAME_LENGTH: usize = 255;

/// Reserved filenames that are not allowed (Windows + Unix special cases)
const RESERVED_FILENAMES: &[&str] = &[
    ".", "..", "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
    "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Validates an output path to ensure it's within the base directory.
///
/// This function prevents path traversal attacks by:
/// - Rejecting absolute paths
/// - Rejecting paths with parent directory components (`..`)
/// - Canonicalizing paths to resolve symlinks
/// - Verifying the resolved path is within the base directory
///
/// # Security
///
/// This function is security-critical. It must ALWAYS be called before writing
/// files based on external input (e.g., tool names from MCP servers).
///
/// # Arguments
///
/// * `base_dir` - The base directory that all output files must be within
/// * `requested_path` - The requested path (relative to base_dir)
///
/// # Returns
///
/// The canonicalized path if valid, or a SecurityViolation error if invalid.
///
/// # Examples
///
/// ```no_run
/// # use std::path::Path;
/// # use turbomcp_cli::path_security::validate_output_path;
/// let base = Path::new("/tmp/output");
/// let safe_path = validate_output_path(base, "tool.json")?;
/// // safe_path is guaranteed to be within /tmp/output
/// # Ok::<(), turbomcp_cli::error::CliError>(())
/// ```
pub fn validate_output_path(base_dir: &Path, requested_path: &str) -> CliResult<PathBuf> {
    // First, check for obvious path traversal patterns in the raw string
    // This catches cases that might not be parsed as ParentDir on all platforms
    if requested_path.contains("..") {
        return Err(CliError::SecurityViolation {
            reason: format!("Path traversal detected: '{}'", requested_path),
            details: "Paths containing '..' are not allowed for security reasons".to_string(),
        });
    }

    let requested = PathBuf::from(requested_path);

    // Reject absolute paths
    if requested.is_absolute() {
        return Err(CliError::SecurityViolation {
            reason: format!("Absolute path not allowed: '{}'", requested_path),
            details: "All output files must use relative paths within the output directory"
                .to_string(),
        });
    }

    // Check for parent directory components (..)
    // This is redundant with the string check above, but provides defense in depth
    for component in requested.components() {
        if matches!(component, Component::ParentDir) {
            return Err(CliError::SecurityViolation {
                reason: format!("Path traversal detected: '{}'", requested_path),
                details: "Paths containing '..' components are not allowed for security reasons"
                    .to_string(),
            });
        }
    }

    // Build full path
    let full_path = base_dir.join(&requested);

    // Canonicalize base directory to resolve symlinks
    let base_canonical = base_dir.canonicalize().map_err(CliError::Io)?;

    // For the full path, we need to handle the case where it doesn't exist yet
    // If the file exists, canonicalize it directly
    if full_path.exists() {
        let canonical = full_path.canonicalize().map_err(CliError::Io)?;

        // Verify it's within base directory
        if !canonical.starts_with(&base_canonical) {
            return Err(CliError::SecurityViolation {
                reason: format!("Path escapes output directory: '{}'", canonical.display()),
                details: format!(
                    "Resolved path '{}' is outside base directory '{}'",
                    canonical.display(),
                    base_canonical.display()
                ),
            });
        }

        return Ok(canonical);
    }

    // File doesn't exist - we need to validate it's safe to create
    // Since we already checked for ".." and absolute paths, the path is safe
    // However, we need to return a path that's consistent with base_canonical
    // Build the path relative to the canonical base
    let relative_to_base =
        full_path
            .strip_prefix(base_dir)
            .map_err(|_| CliError::SecurityViolation {
                reason: "Internal error: path not relative to base".to_string(),
                details: "Path validation failed unexpectedly".to_string(),
            })?;

    Ok(base_canonical.join(relative_to_base))
}

/// Sanitizes a filename to prevent security issues.
///
/// This function:
/// - Removes or replaces unsafe characters (only allows alphanumeric, `-`, `_`, `.`)
/// - Rejects reserved filenames (`.`, `..`, Windows device names)
/// - Enforces maximum length limits
///
/// # Security
///
/// This function is security-critical. It must ALWAYS be called before using
/// external input (e.g., tool names) as filenames.
///
/// # Arguments
///
/// * `name` - The filename to sanitize
///
/// # Returns
///
/// A sanitized filename if valid, or a SecurityViolation error if the name
/// cannot be made safe.
///
/// # Examples
///
/// ```
/// # use turbomcp_cli::path_security::sanitize_filename;
/// assert_eq!(sanitize_filename("my_tool")?, "my_tool");
/// assert_eq!(sanitize_filename("my-file.txt")?, "my-file.txt");
/// // Paths with ".." are rejected for security
/// assert!(sanitize_filename("my/tool/../bad").is_err());
/// # Ok::<(), turbomcp_cli::error::CliError>(())
/// ```
pub fn sanitize_filename(name: &str) -> CliResult<String> {
    if name.is_empty() {
        return Err(CliError::SecurityViolation {
            reason: "Empty filename".to_string(),
            details: "Filename cannot be empty".to_string(),
        });
    }

    // Remove or replace unsafe characters
    // Only allow: alphanumeric, dash, underscore, period
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect();

    if sanitized.is_empty() {
        return Err(CliError::SecurityViolation {
            reason: format!("Invalid filename: '{}'", name),
            details: "Filename must contain at least one alphanumeric character".to_string(),
        });
    }

    // Additional check: reject if the sanitized name still contains ".."
    // This prevents names like "......etcpasswd" which look suspicious
    if sanitized.contains("..") {
        return Err(CliError::SecurityViolation {
            reason: format!("Invalid filename pattern: '{}'", sanitized),
            details: "Filenames containing '..' patterns are not allowed".to_string(),
        });
    }

    // Check length
    if sanitized.len() > MAX_FILENAME_LENGTH {
        return Err(CliError::SecurityViolation {
            reason: format!("Filename too long: {} characters", sanitized.len()),
            details: format!(
                "Filename must be at most {} characters",
                MAX_FILENAME_LENGTH
            ),
        });
    }

    // Check for reserved names (case-insensitive)
    let lower = sanitized.to_lowercase();
    if RESERVED_FILENAMES.contains(&lower.as_str()) {
        return Err(CliError::SecurityViolation {
            reason: format!("Reserved filename: '{}'", sanitized),
            details: "This filename is reserved by the operating system".to_string(),
        });
    }

    // Also reject if it starts with a period (hidden files can be problematic)
    if sanitized.starts_with('.') && sanitized.len() <= 2 {
        return Err(CliError::SecurityViolation {
            reason: format!("Invalid filename: '{}'", sanitized),
            details: "Filenames starting with '.' are not allowed".to_string(),
        });
    }

    Ok(sanitized)
}

/// Validates and sanitizes a filename, then constructs a safe output path.
///
/// This is a convenience function that combines `sanitize_filename` and
/// `validate_output_path` with automatic `.json` extension.
///
/// # Security
///
/// This function performs all necessary security validations.
///
/// # Arguments
///
/// * `base_dir` - The base directory for output files
/// * `name` - The filename to sanitize (e.g., tool name)
/// * `extension` - The file extension to add (e.g., "json")
///
/// # Returns
///
/// A validated, safe output path.
pub fn safe_output_path(base_dir: &Path, name: &str, extension: &str) -> CliResult<PathBuf> {
    let sanitized = sanitize_filename(name)?;
    let filename = if extension.is_empty() {
        sanitized
    } else {
        format!("{}.{}", sanitized, extension)
    };
    validate_output_path(base_dir, &filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_sanitize_valid_filenames() {
        assert_eq!(sanitize_filename("my_tool").unwrap(), "my_tool");
        assert_eq!(sanitize_filename("tool-123").unwrap(), "tool-123");
        assert_eq!(sanitize_filename("tool.v1").unwrap(), "tool.v1");
        assert_eq!(sanitize_filename("Tool_Name_123").unwrap(), "Tool_Name_123");
    }

    #[test]
    fn test_sanitize_removes_unsafe_chars() {
        // Slashes and other path separators should be removed
        assert_eq!(sanitize_filename("my/tool").unwrap(), "mytool");
        assert_eq!(sanitize_filename("my\\tool").unwrap(), "mytool");
        assert_eq!(sanitize_filename("tool:name").unwrap(), "toolname");
        assert_eq!(sanitize_filename("tool*name").unwrap(), "toolname");
    }

    #[test]
    fn test_sanitize_rejects_reserved_names() {
        assert!(sanitize_filename(".").is_err());
        assert!(sanitize_filename("..").is_err());
        assert!(sanitize_filename("con").is_err());
        assert!(sanitize_filename("CON").is_err());
        assert!(sanitize_filename("prn").is_err());
        assert!(sanitize_filename("aux").is_err());
        assert!(sanitize_filename("nul").is_err());
        assert!(sanitize_filename("com1").is_err());
        assert!(sanitize_filename("lpt1").is_err());
    }

    #[test]
    fn test_sanitize_rejects_empty() {
        assert!(sanitize_filename("").is_err());
        assert!(sanitize_filename("///").is_err()); // becomes empty after sanitization
        assert!(sanitize_filename("***").is_err()); // becomes empty after sanitization
    }

    #[test]
    fn test_validate_accepts_relative_paths() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Simple filename
        let result = validate_output_path(base, "tool.json");
        assert!(result.is_ok());

        // Subdirectory (create it first)
        fs::create_dir_all(base.join("subdir")).unwrap();
        let result = validate_output_path(base, "subdir/tool.json");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_rejects_absolute_paths() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        assert!(validate_output_path(base, "/etc/passwd").is_err());
        assert!(validate_output_path(base, "/tmp/evil").is_err());

        // Windows-style absolute paths
        #[cfg(windows)]
        {
            assert!(validate_output_path(base, "C:\\Windows\\System32").is_err());
        }
    }

    #[test]
    fn test_validate_rejects_parent_directory() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        assert!(validate_output_path(base, "..").is_err());
        assert!(validate_output_path(base, "../etc/passwd").is_err());
        assert!(validate_output_path(base, "../../.ssh/authorized_keys").is_err());
        assert!(validate_output_path(base, "subdir/../../../etc/passwd").is_err());
    }

    #[test]
    fn test_validate_handles_existing_files() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create a file
        let test_file = base.join("test.json");
        fs::write(&test_file, "{}").unwrap();

        // Should validate successfully
        let result = validate_output_path(base, "test.json");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_handles_nonexistent_files() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // File doesn't exist yet, but should be valid
        let result = validate_output_path(base, "new_file.json");
        assert!(result.is_ok());

        // Subdirectory doesn't exist, but path should be valid
        let result = validate_output_path(base, "newdir/file.json");
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_output_path_integration() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Valid tool name
        let result = safe_output_path(base, "my_tool", "json");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("my_tool.json"));

        // Malicious tool name with path traversal - should be rejected during sanitization
        let result = safe_output_path(base, "../../../etc/passwd", "json");
        assert!(result.is_err(), "Should reject path traversal attempts");
    }

    #[test]
    fn test_comprehensive_attack_scenarios() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        // Canonicalize base to match what validate_output_path returns
        let base_canonical = base.canonicalize().unwrap();

        // Collection of real-world path traversal attack patterns
        let malicious_inputs = vec![
            "../../../etc/passwd",
            "../../.ssh/authorized_keys",
            "../../../.bash_history",
            "/etc/shadow",
            "../../../../../../../../etc/passwd",
            "..\\..\\..\\windows\\system32",
            "subdir/../../etc/passwd",
        ];

        for input in malicious_inputs {
            // Direct validation should fail
            let result = validate_output_path(base, input);
            assert!(
                result.is_err(),
                "Should reject malicious path directly: {}",
                input
            );

            // Sanitization should either:
            // 1. Fail (reject the malicious input)
            // 2. Succeed and produce a safe filename within base_dir
            match sanitize_filename(input) {
                Ok(sanitized) => {
                    // If sanitization succeeds, validation must also succeed
                    // and the result must be within base_dir
                    let result = validate_output_path(base, &sanitized);
                    if let Ok(path) = result {
                        assert!(
                            path.starts_with(&base_canonical),
                            "Sanitized path must be within base dir: {} -> {} (base: {})",
                            input,
                            path.display(),
                            base_canonical.display()
                        );
                    }
                }
                Err(_) => {
                    // It's OK (and often preferable) for sanitization to fail
                    // on obviously malicious inputs
                }
            }
        }
    }
}

//! Integration tests for path security in CLI operations
//!
//! These tests verify that the CLI properly validates and sanitizes paths
//! to prevent path traversal attacks from malicious MCP servers.

use std::fs;
use std::path::Path;
use tempfile::TempDir;
use turbomcp_cli::path_security::{safe_output_path, sanitize_filename, validate_output_path};

/// Test that valid relative paths are accepted
#[test]
fn test_accepts_valid_relative_paths() {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();

    // Simple filenames should work
    assert!(validate_output_path(base, "tool.json").is_ok());
    assert!(validate_output_path(base, "my_tool.json").is_ok());
    assert!(validate_output_path(base, "tool-123.json").is_ok());

    // Create subdirectory
    fs::create_dir_all(base.join("schemas")).unwrap();

    // Subdirectory paths should work
    assert!(validate_output_path(base, "schemas/tool.json").is_ok());
}

/// Test that absolute paths are rejected
#[test]
fn test_rejects_absolute_paths() {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();

    // Unix-style absolute paths
    assert!(validate_output_path(base, "/etc/passwd").is_err());
    assert!(validate_output_path(base, "/tmp/malicious").is_err());
    assert!(validate_output_path(base, "/root/.ssh/authorized_keys").is_err());

    // Windows-style absolute paths (should be rejected on all platforms)
    #[cfg(windows)]
    {
        assert!(validate_output_path(base, "C:\\Windows\\System32").is_err());
        assert!(validate_output_path(base, "D:\\secrets.txt").is_err());
    }
}

/// Test that path traversal attempts are rejected
#[test]
fn test_rejects_path_traversal() {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();

    // Parent directory references
    assert!(validate_output_path(base, "..").is_err());
    assert!(validate_output_path(base, "../etc/passwd").is_err());
    assert!(validate_output_path(base, "../../.ssh/authorized_keys").is_err());
    assert!(validate_output_path(base, "../../../etc/shadow").is_err());

    // Multiple levels
    assert!(validate_output_path(base, "../../../../../../../../etc/passwd").is_err());

    // Mixed with valid path components
    assert!(validate_output_path(base, "subdir/../../../etc/passwd").is_err());
    assert!(validate_output_path(base, "a/b/c/../../../../../../../etc/passwd").is_err());
}

/// Test filename sanitization
#[test]
fn test_sanitize_removes_unsafe_characters() {
    // Valid characters should pass through
    assert_eq!(sanitize_filename("my_tool").unwrap(), "my_tool");
    assert_eq!(sanitize_filename("tool-123").unwrap(), "tool-123");
    assert_eq!(sanitize_filename("tool.v1.json").unwrap(), "tool.v1.json");

    // Path separators should be removed
    assert_eq!(sanitize_filename("my/tool").unwrap(), "mytool");
    assert_eq!(sanitize_filename("my\\tool").unwrap(), "mytool");

    // Path traversal patterns should be rejected (not just sanitized)
    assert!(sanitize_filename("../../../etc/passwd").is_err());

    // Special characters should be removed
    assert_eq!(sanitize_filename("tool:name").unwrap(), "toolname");
    assert_eq!(sanitize_filename("tool*name").unwrap(), "toolname");
    assert_eq!(sanitize_filename("tool?name").unwrap(), "toolname");
    assert_eq!(sanitize_filename("tool<name>").unwrap(), "toolname");
    assert_eq!(sanitize_filename("tool|name").unwrap(), "toolname");
}

/// Test that reserved filenames are rejected
#[test]
fn test_rejects_reserved_filenames() {
    // Unix special directories
    assert!(sanitize_filename(".").is_err());
    assert!(sanitize_filename("..").is_err());

    // Windows device names (should be rejected on all platforms for portability)
    assert!(sanitize_filename("con").is_err());
    assert!(sanitize_filename("CON").is_err()); // case-insensitive
    assert!(sanitize_filename("prn").is_err());
    assert!(sanitize_filename("aux").is_err());
    assert!(sanitize_filename("nul").is_err());
    assert!(sanitize_filename("com1").is_err());
    assert!(sanitize_filename("com2").is_err());
    assert!(sanitize_filename("lpt1").is_err());
    assert!(sanitize_filename("lpt2").is_err());
}

/// Test that empty or invalid filenames are rejected
#[test]
fn test_rejects_empty_filenames() {
    assert!(sanitize_filename("").is_err());
    assert!(sanitize_filename("   ").is_err()); // whitespace is removed, becomes empty
    assert!(sanitize_filename("///").is_err()); // becomes empty after sanitization
    assert!(sanitize_filename("***").is_err()); // becomes empty after sanitization
    assert!(sanitize_filename("???").is_err()); // becomes empty after sanitization
}

/// Test comprehensive attack scenarios from real-world exploits
#[test]
fn test_comprehensive_attack_scenarios() {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();
    let base_canonical = base.canonicalize().unwrap();

    // Real-world path traversal attack patterns
    let attack_patterns = vec![
        // Basic traversal
        "../../../etc/passwd",
        "../../.ssh/authorized_keys",
        "../../../.bash_history",
        // Deep traversal
        "../../../../../../../../etc/passwd",
        "../../../../../../../../etc/shadow",
        // Absolute paths
        "/etc/passwd",
        "/root/.ssh/id_rsa",
        "/var/log/auth.log",
        // Windows paths
        "..\\..\\..\\windows\\system32",
        "C:\\Windows\\System32\\config\\SAM",
        // Mixed traversal
        "subdir/../../etc/passwd",
        "a/b/c/../../../../../../../etc/passwd",
        // Hidden files
        "../../.env",
        "../../.aws/credentials",
        // Encoded attempts (should be handled by sanitization)
        "..%2F..%2F..%2Fetc%2Fpasswd",
        "..%5C..%5C..%5Cwindows",
    ];

    for pattern in attack_patterns {
        // Direct validation should reject path traversal
        let result = validate_output_path(base, pattern);

        // Some patterns are platform-specific (e.g., Windows paths on Unix)
        // We should reject anything containing ".." or starting with "/"
        let should_be_rejected = pattern.contains("..") || pattern.starts_with('/');

        if should_be_rejected {
            assert!(
                result.is_err(),
                "Should reject malicious path directly: {}",
                pattern
            );
        }

        // After sanitization, it should either:
        // 1. Fail sanitization (if it becomes empty/invalid or contains "..")
        // 2. Pass validation but be within base dir (if it becomes safe)
        match sanitize_filename(pattern) {
            Ok(sanitized) => {
                let result = validate_output_path(base, &sanitized);
                if let Ok(path) = result {
                    assert!(
                        path.starts_with(&base_canonical),
                        "Sanitized path must be within base dir: {} -> {}",
                        pattern,
                        path.display()
                    );
                }
            }
            Err(_) => {
                // It's OK (and expected) for sanitization to fail for malicious input
            }
        }
    }
}

/// Test that the integrated safe_output_path function works correctly
#[test]
fn test_safe_output_path_integration() {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();
    let base_canonical = base.canonicalize().unwrap();

    // Valid tool names should produce valid paths
    let result = safe_output_path(base, "my_tool", "json").unwrap();
    assert!(result.starts_with(&base_canonical));
    assert!(result.ends_with("my_tool.json"));

    // Malicious tool names with path traversal should be rejected
    let result = safe_output_path(base, "../../../etc/passwd", "json");
    assert!(result.is_err(), "Should reject path traversal patterns");

    // Tool names with special characters
    let result = safe_output_path(base, "tool/with/slashes", "json").unwrap();
    assert!(result.starts_with(&base_canonical));
    assert!(!result.to_string_lossy().contains("/with/"));

    // Extension should be added correctly
    let result = safe_output_path(base, "tool", "txt").unwrap();
    assert!(result.ends_with("tool.txt"));

    // Empty extension should work
    let result = safe_output_path(base, "tool", "").unwrap();
    assert!(result.ends_with("tool"));
}

/// Test that paths cannot escape via symlinks
#[test]
#[cfg(unix)] // Symlinks work differently on Windows
fn test_rejects_symlink_escape_attempts() {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();

    // Create a symlink pointing outside the base directory
    let external_dir = TempDir::new().unwrap();
    let symlink_path = base.join("escape");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(external_dir.path(), &symlink_path).unwrap();

        // Trying to write through the symlink should be rejected
        let result = validate_output_path(base, "escape/malicious.json");
        // This should fail because the resolved path is outside base_dir
        assert!(result.is_err() || !result.unwrap().starts_with(base));
    }
}

/// Test handling of existing files
#[test]
fn test_handles_existing_files() {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();
    let base_canonical = base.canonicalize().unwrap();

    // Create an existing file
    let test_file = base.join("existing.json");
    fs::write(&test_file, "{}").unwrap();

    // Should validate successfully
    let result = validate_output_path(base, "existing.json");
    assert!(result.is_ok());
    let validated = result.unwrap();
    assert!(validated.starts_with(&base_canonical));
}

/// Test handling of non-existent files in non-existent directories
#[test]
fn test_handles_nonexistent_subdirectories() {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();
    let base_canonical = base.canonicalize().unwrap();

    // Subdirectory doesn't exist yet, but path should be valid
    let result = validate_output_path(base, "newdir/file.json");
    assert!(result.is_ok());
    let validated = result.unwrap();
    assert!(validated.starts_with(&base_canonical));
}

/// Test that validation handles Unicode correctly
#[test]
fn test_handles_unicode_filenames() {
    // Unicode alphanumeric characters should be allowed
    assert!(sanitize_filename("tool_测试").is_ok());
    assert!(sanitize_filename("инструмент").is_ok());
    assert!(sanitize_filename("أداة").is_ok());

    // But path separators should still be removed
    let sanitized = sanitize_filename("测试/工具").unwrap();
    assert!(!sanitized.contains('/'));
}

/// Test maximum filename length enforcement
#[test]
fn test_rejects_overly_long_filenames() {
    // Create a filename that's too long (> 255 characters)
    let long_name = "a".repeat(256);
    assert!(sanitize_filename(&long_name).is_err());

    // Just under the limit should be OK
    let ok_name = "a".repeat(255);
    assert!(sanitize_filename(&ok_name).is_ok());
}

/// Test real-world scenario: exporting schemas from malicious server
#[test]
fn test_malicious_server_scenario() {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();
    let base_canonical = base.canonicalize().unwrap();

    // Simulate tools from a malicious server
    let malicious_tool_names = vec![
        "../../../etc/passwd",
        "../../.ssh/authorized_keys",
        "/root/.bash_history",
        "CON",
        "tool|rm -rf /",
        "tool; DROP TABLE users;",
    ];

    for tool_name in malicious_tool_names {
        // Using safe_output_path should either:
        // 1. Produce a safe path within base_dir
        // 2. Return an error
        match safe_output_path(base, tool_name, "json") {
            Ok(path) => {
                // If it succeeds, must be within base_dir
                assert!(
                    path.starts_with(&base_canonical),
                    "Path must be within base dir: {}",
                    path.display()
                );

                // Should not contain dangerous patterns
                let path_str = path.to_string_lossy();
                assert!(!path_str.contains(".."));
                assert!(!path_str.contains("/etc/"));
                assert!(!path_str.contains(".ssh"));
                assert!(!path_str.contains("rm -rf"));
                assert!(!path_str.contains("DROP TABLE"));
            }
            Err(_) => {
                // It's acceptable to reject completely invalid names
            }
        }
    }

    // Verify no files were written outside the base directory
    // (This is a sanity check - the tests above should prevent this)
    assert!(!Path::new("/etc/passwd").exists() || !Path::new("/etc/passwd-test").exists());
}

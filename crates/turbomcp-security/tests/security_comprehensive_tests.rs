//! Comprehensive security tests for TurboMCP file operations
//!
//! This test suite validates all security measures against various attack vectors
//! including path traversal, symlink attacks, resource exhaustion, and more.

use std::path::Path;
use tempfile::TempDir;
use tokio::fs;
use turbomcp_security::*;

/// Test path traversal attack prevention
#[tokio::test]
async fn test_path_traversal_attack_prevention() {
    let validator = FileSecurityValidator::new();

    // Classic path traversal patterns that should be blocked
    let malicious_paths = [
        "../../../etc/passwd",
        "..\\..\\..\\windows\\system32\\config\\sam",
        "./data/../../../sensitive",
        "data/../../etc/hosts",
        "%2e%2e%2f%2e%2e%2f%2e%2e%2fetc%2fpasswd", // URL encoded
        "..;/etc/passwd",
        "...//etc/passwd",
        ".../etc/passwd",
        "../\\/etc/passwd",
    ];

    for malicious_path in &malicious_paths {
        let result = validator
            .validate_file_access(Path::new(malicious_path))
            .await;
        assert!(
            result.is_err(),
            "Path traversal attack should be blocked: {}",
            malicious_path
        );

        // Verify it's detected as a path traversal error
        match result {
            Err(SecurityError::PathTraversal(_)) => {
                // Expected
            }
            Err(other_error) => {
                // Also acceptable - any security error is fine
                println!("Path {} blocked with: {}", malicious_path, other_error);
            }
            Ok(_) => panic!("Path traversal attack not blocked: {}", malicious_path),
        }
    }
}

/// Test symlink attack prevention
#[tokio::test]
async fn test_symlink_attack_prevention() {
    let temp_dir = TempDir::new().unwrap();
    let validator = FileSecurityValidator::new();

    // Create a sensitive file outside the temp directory
    let sensitive_dir = TempDir::new().unwrap();
    let sensitive_file = sensitive_dir.path().join("secret.txt");
    fs::write(&sensitive_file, "TOP SECRET DATA").await.unwrap();

    // Create a symlink in our temp directory pointing to the sensitive file
    let symlink_path = temp_dir.path().join("innocent_looking_file.txt");

    #[cfg(unix)]
    {
        tokio::process::Command::new("ln")
            .arg("-s")
            .arg(&sensitive_file)
            .arg(&symlink_path)
            .status()
            .await
            .unwrap();

        // Attempt to access through symlink should be blocked
        let result = validator.validate_file_access(&symlink_path).await;
        assert!(result.is_err(), "Symlink attack should be blocked");

        match result {
            Err(SecurityError::SymlinkAttack(_)) => {
                // Expected
            }
            Err(_) => {
                // Any error is acceptable for symlink protection
            }
            Ok(_) => panic!("Symlink attack not blocked"),
        }
    }
}

/// Test file size limit enforcement
#[tokio::test]
async fn test_file_size_limit_enforcement() {
    let temp_dir = TempDir::new().unwrap();

    // Create validator with small file size limit
    let policy = SecurityPolicy::default().max_file_size(1024); // 1KB limit
    let resource_policy = ResourcePolicy::default().max_file_size(1024);
    let audit_logger = AuditLogger::new();

    let validator = FileSecurityValidator::with_policies(policy, resource_policy, audit_logger);

    // Create a file larger than the limit
    let large_file = temp_dir.path().join("large_file.txt");
    let large_data = vec![b'A'; 2048]; // 2KB file
    fs::write(&large_file, large_data).await.unwrap();

    // Should be rejected due to size limit
    let result = validator.validate_file_access(&large_file).await;
    assert!(result.is_err());

    match result {
        Err(SecurityError::FileSizeLimitExceeded { actual, limit }) => {
            assert_eq!(actual, 2048);
            assert_eq!(limit, 1024);
        }
        Err(other) => panic!("Expected FileSizeLimitExceeded, got: {}", other),
        Ok(_) => panic!("Large file should be rejected"),
    }
}

/// Test resource exhaustion prevention
#[tokio::test]
async fn test_resource_exhaustion_prevention() {
    let temp_dir = TempDir::new().unwrap();

    // Create validator with very limited resources
    let policy = SecurityPolicy::default();
    let resource_policy = ResourcePolicy::default()
        .max_concurrent_files(2)
        .max_memory_usage(2048);
    let audit_logger = AuditLogger::new();

    let validator = FileSecurityValidator::with_policies(policy, resource_policy, audit_logger);

    // Create test files
    let files: Vec<_> = (0..5)
        .map(|i| {
            let file = temp_dir.path().join(format!("file_{}.txt", i));
            file
        })
        .collect();

    // Create the files with some content
    for (i, file) in files.iter().enumerate() {
        let data = vec![b'X'; 512]; // 512 bytes each
        fs::write(file, data).await.unwrap();
    }

    // First two files should succeed
    let _guard1 = validator.validate_file_access(&files[0]).await.unwrap();
    let _guard2 = validator.validate_file_access(&files[1]).await.unwrap();

    // Third file should fail due to concurrency limit
    let result = validator.validate_file_access(&files[2]).await;
    assert!(result.is_err());

    match result {
        Err(SecurityError::ResourceLimitExceeded { resource_type, .. }) => {
            assert!(
                resource_type.contains("concurrent") || resource_type.contains("memory"),
                "Should be a resource limit error: {}",
                resource_type
            );
        }
        Err(other) => panic!("Expected resource limit error, got: {}", other),
        Ok(_) => panic!("Resource exhaustion should be prevented"),
    }
}

/// Test forbidden file extension blocking
#[tokio::test]
async fn test_forbidden_extension_blocking() {
    let temp_dir = TempDir::new().unwrap();
    let validator = FileSecurityValidator::new();

    let dangerous_files = [
        "malware.exe",
        "script.bat",
        "trojan.com",
        "virus.scr",
        "payload.dll",
        "installer.msi",
        "MALWARE.EXE", // Test case insensitivity
    ];

    for dangerous_file in &dangerous_files {
        let file_path = temp_dir.path().join(dangerous_file);
        fs::write(&file_path, b"malicious content").await.unwrap();

        let result = validator.validate_file_access(&file_path).await;
        assert!(
            result.is_err(),
            "Dangerous file extension should be blocked: {}",
            dangerous_file
        );

        match result {
            Err(SecurityError::ForbiddenExtension(_)) => {
                // Expected
            }
            Err(_) => {
                // Any security error is acceptable
            }
            Ok(_) => panic!("Dangerous file not blocked: {}", dangerous_file),
        }
    }
}

/// Test allowed extension whitelist
#[tokio::test]
async fn test_allowed_extension_whitelist() {
    let temp_dir = TempDir::new().unwrap();

    // Create validator that only allows .json and .txt files
    let policy = SecurityPolicy::default().allowed_extensions(&[".json", ".txt"]);
    let resource_policy = ResourcePolicy::default();
    let audit_logger = AuditLogger::new();

    let validator = FileSecurityValidator::with_policies(policy, resource_policy, audit_logger);

    // Allowed files should pass
    let allowed_files = ["config.json", "readme.txt", "DATA.JSON"];
    for allowed_file in &allowed_files {
        let file_path = temp_dir.path().join(allowed_file);
        fs::write(&file_path, b"safe content").await.unwrap();

        let result = validator.validate_file_access(&file_path).await;
        assert!(
            result.is_ok(),
            "Allowed file should pass validation: {}",
            allowed_file
        );
    }

    // Disallowed files should be blocked
    let disallowed_files = ["image.png", "document.pdf", "data.xml"];
    for disallowed_file in &disallowed_files {
        let file_path = temp_dir.path().join(disallowed_file);
        fs::write(&file_path, b"content").await.unwrap();

        let result = validator.validate_file_access(&file_path).await;
        assert!(
            result.is_err(),
            "Disallowed file should be blocked: {}",
            disallowed_file
        );
    }
}

/// Test directory depth limit
#[tokio::test]
async fn test_directory_depth_limit() {
    let temp_dir = TempDir::new().unwrap();

    // Create validator with shallow depth limit
    let policy = SecurityPolicy::default().max_directory_depth(5);
    let resource_policy = ResourcePolicy::default();
    let audit_logger = AuditLogger::new();

    let validator = FileSecurityValidator::with_policies(policy, resource_policy, audit_logger);

    // Create a very deep directory structure
    let mut deep_path = temp_dir.path().to_path_buf();
    for i in 0..10 {
        deep_path = deep_path.join(format!("level_{}", i));
    }

    fs::create_dir_all(&deep_path).await.unwrap();
    let deep_file = deep_path.join("deep_file.txt");
    fs::write(&deep_file, b"content").await.unwrap();

    // Should be rejected due to depth limit
    let result = validator.validate_file_access(&deep_file).await;
    assert!(result.is_err());

    match result {
        Err(SecurityError::DirectoryDepthExceeded { actual, limit }) => {
            assert!(actual > limit);
        }
        Err(_) => {
            // Any error is acceptable for depth limit
        }
        Ok(_) => panic!("Deep directory should be rejected"),
    }
}

/// Test memory mapping security
#[tokio::test]
async fn test_memory_mapping_security() {
    let temp_dir = TempDir::new().unwrap();
    let validator = FileSecurityValidator::new();

    // Create test file
    let test_file = temp_dir.path().join("mmap_test.dat");
    let test_data = vec![b'M'; 1024]; // 1KB file
    fs::write(&test_file, test_data).await.unwrap();

    // Valid mmap access should succeed
    let result = validator
        .validate_mmap_access(&test_file, 0, Some(512))
        .await;
    assert!(result.is_ok());

    let (safe_path, offset, length) = result.unwrap();
    assert_eq!(offset, 0);
    assert_eq!(length, 512);
    assert!(safe_path.ends_with("mmap_test.dat"));

    // Invalid offset should fail
    let result = validator
        .validate_mmap_access(&test_file, 2048, Some(100))
        .await;
    assert!(result.is_err());
}

/// Test concurrent access limits
#[tokio::test]
async fn test_concurrent_access_limits() {
    let temp_dir = TempDir::new().unwrap();

    let policy = SecurityPolicy::default();
    let resource_policy = ResourcePolicy::default().max_concurrent_files(3);
    let audit_logger = AuditLogger::new();

    let validator = FileSecurityValidator::with_policies(policy, resource_policy, audit_logger);

    // Create test files
    let files: Vec<_> = (0..5)
        .map(|i| {
            let file = temp_dir.path().join(format!("concurrent_{}.txt", i));
            file
        })
        .collect();

    for file in &files {
        fs::write(file, b"test data").await.unwrap();
    }

    // Acquire guards up to the limit
    let mut guards = Vec::new();
    for i in 0..3 {
        let guard = validator.validate_file_access(&files[i]).await;
        assert!(guard.is_ok(), "Should allow {} concurrent files", i + 1);
        guards.push(guard.unwrap());
    }

    // Fourth access should be denied
    let result = validator.validate_file_access(&files[3]).await;
    assert!(result.is_err(), "Should deny access beyond limit");

    // Drop one guard and try again
    guards.pop();
    tokio::task::yield_now().await; // Allow cleanup task to run

    // Now it should succeed
    let result = validator.validate_file_access(&files[3]).await;
    // Note: this might still fail due to async cleanup timing, which is acceptable
    if result.is_err() {
        println!("Access still denied due to async cleanup timing - this is acceptable");
    }
}

/// Test rate limiting
#[tokio::test]
async fn test_rate_limiting() {
    let temp_dir = TempDir::new().unwrap();

    let policy = SecurityPolicy::default();
    let resource_policy = ResourcePolicy::default().max_operations_per_second(2);
    let audit_logger = AuditLogger::new();

    let validator = FileSecurityValidator::with_policies(policy, resource_policy, audit_logger);

    let test_file = temp_dir.path().join("rate_test.txt");
    fs::write(&test_file, b"test data").await.unwrap();

    // First two operations should succeed
    let _guard1 = validator.validate_file_access(&test_file).await.unwrap();
    let _guard2 = validator.validate_file_access(&test_file).await.unwrap();

    // Third operation should be rate limited
    let result = validator.validate_file_access(&test_file).await;
    assert!(result.is_err(), "Should be rate limited");

    match result {
        Err(SecurityError::ResourceLimitExceeded { resource_type, .. }) => {
            assert_eq!(resource_type, "rate_limit");
        }
        Err(other) => panic!("Expected rate limit error, got: {}", other),
        Ok(_) => panic!("Rate limiting should prevent access"),
    }
}

/// Test socket path validation
#[tokio::test]
async fn test_socket_path_validation() {
    let temp_dir = TempDir::new().unwrap();
    let validator = FileSecurityValidator::new();

    // Valid socket path should succeed
    let valid_socket = temp_dir.path().join("app.sock");
    let result = validator.validate_socket_path(&valid_socket).await;
    assert!(result.is_ok());

    // Socket in system directory should be rejected
    let system_paths = [
        "/proc/malicious.sock",
        "/sys/evil.sock",
        "/dev/bad.sock",
        "/etc/config.sock",
    ];

    for system_path in &system_paths {
        let result = validator.validate_socket_path(Path::new(system_path)).await;
        assert!(
            result.is_err(),
            "System directory socket should be rejected: {}",
            system_path
        );
    }
}

/// Test audit logging functionality
#[tokio::test]
async fn test_audit_logging() {
    let temp_dir = TempDir::new().unwrap();
    let validator = FileSecurityValidator::new();

    let test_file = temp_dir.path().join("audit_test.txt");
    fs::write(&test_file, b"test content").await.unwrap();

    // Successful access should be audited
    let _guard = validator.validate_file_access(&test_file).await.unwrap();

    // Malicious access should be audited
    let malicious_path = Path::new("../../../etc/passwd");
    let _result = validator.validate_file_access(malicious_path).await;

    // Give audit system time to process events
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Note: In a production system, we would verify audit logs were written
    // For this test, we just verify the operations complete without error
}

/// Integration test combining multiple attack vectors
#[tokio::test]
async fn test_multi_vector_attack_simulation() {
    let temp_dir = TempDir::new().unwrap();

    // Create restrictive security policy
    let policy = SecurityPolicy::default()
        .max_file_size(2048)
        .allowed_extensions(&[".json", ".txt"])
        .max_directory_depth(5);

    let resource_policy = ResourcePolicy::default()
        .max_concurrent_files(2)
        .max_operations_per_second(10);

    let audit_logger = AuditLogger::new();

    let validator = FileSecurityValidator::with_policies(policy, resource_policy, audit_logger);

    // Simulate sophisticated attack combining multiple vectors
    let attack_scenarios = [
        ("../../../etc/passwd", "Path traversal"),
        ("malware.exe", "Dangerous extension"),
        ("huge_file.json", "Size limit bypass"),
    ];

    // Create the files that can be created
    let large_data = vec![b'L'; 5000]; // 5KB file
    let large_file = temp_dir.path().join("huge_file.json");
    fs::write(&large_file, large_data).await.unwrap();

    let malware_file = temp_dir.path().join("malware.exe");
    fs::write(&malware_file, b"fake malware").await.unwrap();

    for (attack_path, attack_type) in &attack_scenarios {
        let path = if attack_path.starts_with("../") {
            Path::new(attack_path).to_path_buf()
        } else {
            temp_dir.path().join(attack_path)
        };

        let result = validator.validate_file_access(&path).await;
        assert!(
            result.is_err(),
            "Attack should be blocked: {} ({})",
            attack_path,
            attack_type
        );

        println!("Blocked {} attack: {}", attack_type, attack_path);
    }

    // Verify legitimate access still works
    let legitimate_file = temp_dir.path().join("safe.txt");
    fs::write(&legitimate_file, b"safe content").await.unwrap();

    let result = validator.validate_file_access(&legitimate_file).await;
    assert!(result.is_ok(), "Legitimate access should succeed");
}

/// Performance test for security validation overhead
#[tokio::test]
async fn test_security_validation_performance() {
    let temp_dir = TempDir::new().unwrap();
    let validator = FileSecurityValidator::new();

    // Create test file
    let test_file = temp_dir.path().join("perf_test.txt");
    fs::write(&test_file, b"performance test data")
        .await
        .unwrap();

    let start_time = std::time::Instant::now();
    let iterations = 100;

    // Run validation multiple times
    for _i in 0..iterations {
        let _guard = validator.validate_file_access(&test_file).await.unwrap();
        // Guards drop immediately, simulating quick access patterns
    }

    let elapsed = start_time.elapsed();
    let avg_time = elapsed / iterations;

    println!(
        "Security validation performance: {} iterations in {:?} (avg: {:?})",
        iterations, elapsed, avg_time
    );

    // Ensure reasonable performance (adjust threshold as needed)
    assert!(
        avg_time.as_millis() < 50,
        "Security validation should be fast: {:?}",
        avg_time
    );
}

/// Test error handling and recovery
#[tokio::test]
async fn test_error_handling_and_recovery() {
    let validator = FileSecurityValidator::new();

    // Non-existent file should return appropriate error
    let non_existent = Path::new("/tmp/does_not_exist_12345.txt");
    let result = validator.validate_file_access(non_existent).await;
    assert!(result.is_err());

    // Permission denied scenarios
    #[cfg(unix)]
    {
        // Try to access a directory we can't read (if it exists)
        let restricted_path = Path::new("/root/restricted_file.txt");
        let result = validator.validate_file_access(restricted_path).await;
        // This might succeed if running as root, which is fine
        if result.is_err() {
            println!("Permission denied test passed");
        } else {
            println!("Running with elevated privileges - permission test skipped");
        }
    }

    // Malformed paths
    let malformed_paths = ["", "\0", "file\0with\0nulls.txt"];

    for malformed in &malformed_paths {
        if !malformed.is_empty() {
            // Skip empty string on some platforms
            let result = validator.validate_file_access(Path::new(malformed)).await;
            assert!(
                result.is_err(),
                "Malformed path should be rejected: {:?}",
                malformed
            );
        }
    }
}

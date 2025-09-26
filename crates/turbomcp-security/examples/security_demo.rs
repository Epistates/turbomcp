//! TurboMCP Security System Demonstration
//!
//! This example shows how to use the TurboMCP security system to protect
//! file operations against various attack vectors.

use std::path::Path;
use tempfile::TempDir;
use tokio::fs;
use turbomcp_security::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🛡️ TurboMCP Security System Demo");
    println!("=====================================\n");

    // Create a temporary directory for our demo
    let temp_dir = TempDir::new()?;
    println!("📁 Demo directory: {:?}\n", temp_dir.path());

    // 1. Basic Security Validation
    demonstrate_basic_validation(&temp_dir).await?;

    // 2. Path Traversal Protection
    demonstrate_path_traversal_protection().await?;

    // 3. Resource Limits
    demonstrate_resource_limits(&temp_dir).await?;

    // 4. File Extension Filtering
    demonstrate_extension_filtering(&temp_dir).await?;

    // 5. Advanced Configuration
    demonstrate_advanced_configuration(&temp_dir).await?;

    // 6. Security Monitoring
    demonstrate_security_monitoring(&temp_dir).await?;

    println!("✅ Security demonstration complete!");
    println!("\n🔒 Your files are now protected by TurboMCP Security!");

    Ok(())
}

/// Demonstrate basic file security validation
async fn demonstrate_basic_validation(
    temp_dir: &TempDir,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("1️⃣ Basic Security Validation");
    println!("-----------------------------");

    // Create a security validator with default settings
    let validator = FileSecurityValidator::new();

    // Create a safe test file
    let safe_file = temp_dir.path().join("safe_data.json");
    fs::write(&safe_file, r#"{"message": "Hello, secure world!"}"#).await?;

    // Validate access to the safe file
    match validator.validate_file_access(&safe_file).await {
        Ok(validated_path) => {
            println!(
                "✅ Safe file access granted: {:?}",
                validated_path.file_name()
            );
        }
        Err(e) => {
            println!("❌ Safe file access denied: {}", e);
        }
    }

    println!();
    Ok(())
}

/// Demonstrate path traversal attack prevention
async fn demonstrate_path_traversal_protection() -> Result<(), Box<dyn std::error::Error>> {
    println!("2️⃣ Path Traversal Attack Prevention");
    println!("------------------------------------");

    let validator = FileSecurityValidator::new();

    // Common path traversal attack patterns
    let malicious_paths = [
        "../../../etc/passwd",
        "..\\..\\..\\windows\\system32\\config\\sam",
        "./data/../../../sensitive/secret.txt",
        "%2e%2e%2f%2e%2e%2f%2e%2e%2fetc%2fpasswd", // URL encoded
        "..;/etc/passwd",
        ".../etc/passwd",
    ];

    println!("Testing common attack patterns:");
    for attack_path in &malicious_paths {
        match validator.validate_file_access(Path::new(attack_path)).await {
            Ok(_) => {
                println!("❌ SECURITY FAILURE: Attack not blocked: {}", attack_path);
            }
            Err(SecurityError::PathTraversal(details)) => {
                println!("✅ Path traversal blocked: {} ({})", attack_path, details);
            }
            Err(other_error) => {
                println!("✅ Attack blocked: {} ({})", attack_path, other_error);
            }
        }
    }

    println!();
    Ok(())
}

/// Demonstrate resource limit enforcement
async fn demonstrate_resource_limits(temp_dir: &TempDir) -> Result<(), Box<dyn std::error::Error>> {
    println!("3️⃣ Resource Limit Enforcement");
    println!("------------------------------");

    // Create a validator with strict resource limits
    let security_policy = SecurityPolicy::default();
    let resource_policy = ResourcePolicy::default()
        .max_file_size(1024) // 1KB limit for demo
        .max_concurrent_files(2); // Only 2 concurrent files

    let audit_logger = AuditLogger::new();
    let validator =
        FileSecurityValidator::with_policies(security_policy, resource_policy, audit_logger);

    // Create files of different sizes
    let small_file = temp_dir.path().join("small.txt");
    let large_file = temp_dir.path().join("large.txt");

    fs::write(&small_file, b"Small file content").await?; // ~18 bytes
    fs::write(&large_file, vec![b'X'; 2048]).await?; // 2KB file

    // Test file size limits
    println!("Testing file size limits:");
    match validator.validate_file_access(&small_file).await {
        Ok(_) => println!("✅ Small file (18 bytes) access granted"),
        Err(e) => println!("❌ Small file access denied: {}", e),
    }

    match validator.validate_file_access(&large_file).await {
        Ok(_) => println!("❌ Large file access should be blocked!"),
        Err(SecurityError::FileSizeLimitExceeded { actual, limit }) => {
            println!(
                "✅ Large file blocked: {} bytes > {} bytes limit",
                actual, limit
            );
        }
        Err(e) => println!("✅ Large file blocked: {}", e),
    }

    // Test concurrent access limits
    println!("\nTesting concurrent access limits:");
    let file1 = temp_dir.path().join("concurrent1.txt");
    let file2 = temp_dir.path().join("concurrent2.txt");
    let file3 = temp_dir.path().join("concurrent3.txt");

    fs::write(&file1, b"file 1").await?;
    fs::write(&file2, b"file 2").await?;
    fs::write(&file3, b"file 3").await?;

    // Acquire guards (these will hold resources)
    let _guard1 = validator.validate_file_access(&file1).await?;
    println!("✅ First concurrent file access granted");

    let _guard2 = validator.validate_file_access(&file2).await?;
    println!("✅ Second concurrent file access granted");

    // Third should fail
    match validator.validate_file_access(&file3).await {
        Ok(_) => println!("❌ Third concurrent access should be blocked!"),
        Err(e) => println!("✅ Third concurrent access blocked: {}", e),
    }

    println!();
    Ok(())
}

/// Demonstrate file extension filtering
async fn demonstrate_extension_filtering(
    temp_dir: &TempDir,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("4️⃣ File Extension Filtering");
    println!("---------------------------");

    // Create validator that only allows specific extensions
    let security_policy = SecurityPolicy::default().allowed_extensions(&[".json", ".txt", ".md"]);

    let resource_policy = ResourcePolicy::default();
    let audit_logger = AuditLogger::new();
    let validator =
        FileSecurityValidator::with_policies(security_policy, resource_policy, audit_logger);

    // Test various file types
    let test_files = [
        ("config.json", "allowed"),
        ("readme.txt", "allowed"),
        ("docs.md", "allowed"),
        ("script.py", "blocked"),
        ("malware.exe", "blocked"),
        ("image.png", "blocked"),
    ];

    println!("Testing file extension filtering:");
    for (filename, expected) in &test_files {
        let file_path = temp_dir.path().join(filename);
        fs::write(&file_path, format!("Content of {}", filename)).await?;

        match validator.validate_file_access(&file_path).await {
            Ok(_) => {
                if *expected == "allowed" {
                    println!("✅ {} - correctly allowed", filename);
                } else {
                    println!("❌ {} - should have been blocked!", filename);
                }
            }
            Err(SecurityError::ForbiddenExtension(details)) => {
                if *expected == "blocked" {
                    println!("✅ {} - correctly blocked ({})", filename, details);
                } else {
                    println!("❌ {} - should have been allowed!", filename);
                }
            }
            Err(e) => {
                println!("✅ {} - blocked with: {}", filename, e);
            }
        }
    }

    println!();
    Ok(())
}

/// Demonstrate advanced security configuration
async fn demonstrate_advanced_configuration(
    temp_dir: &TempDir,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("5️⃣ Advanced Security Configuration");
    println!("----------------------------------");

    // Create a highly restrictive security policy
    let security_policy = SecurityPolicy::default()
        .max_file_size(5 * 1024) // 5KB limit
        .max_directory_depth(5) // Max 5 levels deep
        .allowed_extensions(&[".json", ".txt"])
        .allowed_base_paths(&[temp_dir.path().to_str().unwrap()])
        .forbidden_extensions(&[".exe", ".bat", ".sh"])
        .allow_symlinks(false)
        .require_absolute_paths(false);

    let resource_policy = ResourcePolicy::default()
        .max_concurrent_files(5)
        .max_memory_usage(10 * 1024) // 10KB total memory
        .max_operations_per_second(100);

    let audit_logger = AuditLogger::new();
    let validator =
        FileSecurityValidator::with_policies(security_policy, resource_policy, audit_logger);

    println!("Created highly restrictive validator:");
    println!("  • Max file size: 5KB");
    println!("  • Max directory depth: 5 levels");
    println!("  • Allowed extensions: .json, .txt only");
    println!("  • Symlinks disabled");
    println!("  • Memory limit: 10KB total");
    println!("  • Rate limit: 100 ops/sec");

    // Test the restrictive validator
    let test_file = temp_dir.path().join("restricted_test.json");
    fs::write(&test_file, r#"{"test": "data", "size": "small"}"#).await?;

    match validator.validate_file_access(&test_file).await {
        Ok(_) => println!("✅ File passes restrictive validation"),
        Err(e) => println!("❌ File rejected by restrictive validation: {}", e),
    }

    // Test memory mapping validation
    println!("\nTesting memory mapping security:");
    match validator
        .validate_mmap_access(&test_file, 0, Some(100))
        .await
    {
        Ok((path, offset, length)) => {
            println!(
                "✅ Memory mapping granted: {:?} [{}..{}]",
                path.file_name(),
                offset,
                offset + length
            );
        }
        Err(e) => println!("❌ Memory mapping denied: {}", e),
    }

    println!();
    Ok(())
}

/// Demonstrate security monitoring and audit logging
async fn demonstrate_security_monitoring(
    temp_dir: &TempDir,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("6️⃣ Security Monitoring & Audit Logging");
    println!("--------------------------------------");

    // Create validator with audit logging
    let validator = FileSecurityValidator::new();

    // Create test files
    let good_file = temp_dir.path().join("legitimate.json");
    let bad_file = temp_dir.path().join("malware.exe");

    fs::write(&good_file, r#"{"legitimate": "data"}"#).await?;
    fs::write(&bad_file, b"fake malware").await?;

    println!("Generating security events for monitoring:");

    // Legitimate access (will be logged as successful)
    println!("\n📊 Attempting legitimate file access...");
    match validator.validate_file_access(&good_file).await {
        Ok(_) => println!("✅ Legitimate access granted and logged"),
        Err(e) => println!("❌ Unexpected rejection: {}", e),
    }

    // Malicious access attempts (will be logged as security events)
    println!("\n🚨 Simulating security attacks for monitoring...");

    let attack_attempts = [
        ("../../../etc/passwd", "Path traversal attack"),
        (bad_file.to_str().unwrap(), "Dangerous file extension"),
    ];

    for (attack_path, attack_type) in &attack_attempts {
        match validator.validate_file_access(Path::new(attack_path)).await {
            Ok(_) => println!("❌ SECURITY BREACH: {} succeeded!", attack_type),
            Err(e) => {
                println!("✅ {} blocked: {}", attack_type, e);

                // Check if this is a critical security event
                if e.is_critical() {
                    println!("   🚨 CRITICAL ALERT triggered for {}", attack_type);
                }
            }
        }
    }

    // Wait a moment for audit events to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    println!("\n📈 Security events have been logged to the audit system");
    println!("   In production, these would trigger:");
    println!("   • SIEM system alerts");
    println!("   • Security team notifications");
    println!("   • Automated response procedures");
    println!("   • Compliance audit trails");

    println!();
    Ok(())
}

/// Additional helper function to demonstrate socket path validation
#[allow(dead_code)]
async fn demonstrate_socket_security() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 Socket Path Security");
    println!("----------------------");

    let validator = FileSecurityValidator::new();

    // Test socket paths
    let socket_paths = [
        ("/tmp/app.sock", "should be allowed"),
        ("/proc/malicious.sock", "should be blocked"),
        ("/etc/config.sock", "should be blocked"),
    ];

    for (socket_path, expected) in &socket_paths {
        match validator.validate_socket_path(Path::new(socket_path)).await {
            Ok(_) => println!("✅ {} - {}", socket_path, expected),
            Err(e) => println!("🚫 {} blocked: {}", socket_path, e),
        }
    }

    Ok(())
}

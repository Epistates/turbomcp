# TurboMCP Security Module

[![Crate](https://img.shields.io/crates/v/turbomcp-security.svg)](https://crates.io/crates/turbomcp-security)
[![Documentation](https://docs.rs/turbomcp-security/badge.svg)](https://docs.rs/turbomcp-security)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Production-grade file safety and path validation for TurboMCP. This module provides comprehensive security measures to prevent path traversal attacks, symlink attacks, resource exhaustion, and other file-based vulnerabilities.

## 🚨 Critical Security Features

### Path Validation Security
- **Canonicalization**: Resolves all symbolic links and relative paths
- **Traversal Prevention**: Blocks directory traversal attacks (`../`, symlinks)
- **Path Whitelist/Blacklist**: Configurable allowed and forbidden directories
- **Extension Validation**: File extension allow/deny lists
- **Depth Limits**: Prevents deeply nested directory attacks

### Resource Protection
- **File Size Limits**: Prevents large file DoS attacks
- **Concurrency Limits**: Controls simultaneous file operations
- **Memory Usage Caps**: Limits total memory consumption
- **Rate Limiting**: Operations per second throttling
- **Memory Map Protection**: Special limits for mmap operations

### Audit & Monitoring
- **Comprehensive Logging**: All security events logged
- **Real-time Alerting**: Critical attacks trigger immediate alerts
- **Structured Events**: Machine-readable audit logs
- **Performance Metrics**: Resource usage monitoring

## 🚀 Quick Start

### Basic Usage

```rust
use turbomcp_security::{FileSecurityValidator, SecurityPolicy, ResourcePolicy, AuditLogger};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a security validator with default policies
    let validator = FileSecurityValidator::new();

    // Validate file access
    let safe_path = validator.validate_file_access("/tmp/data.json").await?;
    println!("Safe to access: {:?}", safe_path);

    Ok(())
}
```

### Advanced Configuration

```rust
use turbomcp_security::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure security policy
    let security_policy = SecurityPolicy::default()
        .max_file_size(10 * 1024 * 1024) // 10MB limit
        .allowed_extensions(&[".json", ".txt", ".md"])
        .forbidden_paths(&["/etc", "/proc", "/sys"])
        .allowed_base_paths(&["/tmp", "/var/app"])
        .max_directory_depth(10)
        .allow_symlinks(false);

    // Configure resource limits
    let resource_policy = ResourcePolicy::default()
        .max_concurrent_files(100)
        .max_memory_usage(1024 * 1024 * 1024) // 1GB
        .max_operations_per_second(1000);

    // Set up audit logging
    let audit_config = AuditConfig::default();
    let audit_logger = AuditLogger::with_config(audit_config);

    // Create validator with custom policies
    let validator = FileSecurityValidator::with_policies(
        security_policy,
        resource_policy,
        audit_logger,
    );

    // Use validator for file operations
    match validator.validate_file_access("/tmp/config.json").await {
        Ok(safe_path) => {
            println!("Access granted: {:?}", safe_path);
            // Proceed with file operations
        }
        Err(SecurityError::PathTraversal(details)) => {
            eprintln!("🚨 ATTACK DETECTED: Path traversal - {}", details);
        }
        Err(SecurityError::FileSizeLimitExceeded { actual, limit }) => {
            eprintln!("File too large: {} > {} bytes", actual, limit);
        }
        Err(other_error) => {
            eprintln!("Security error: {}", other_error);
        }
    }

    Ok(())
}
```

## 🛡️ Security Architecture

### Defense in Depth

The security module implements multiple layers of protection:

1. **Input Validation**: All paths are sanitized and validated
2. **Canonicalization**: Resolves symlinks and relative components
3. **Policy Enforcement**: Checks against allow/deny lists
4. **Resource Limiting**: Prevents resource exhaustion
5. **Audit Logging**: All events are logged for monitoring
6. **Real-time Alerting**: Critical events trigger immediate alerts

### Attack Vectors Prevented

#### Path Traversal Attacks
```rust
// These attacks are automatically blocked:
// "../../../etc/passwd"
// "..\\..\\windows\\system32"
// "%2e%2e%2f" (URL encoded)
// "..;/etc/passwd"
// ".../etc/passwd"

let result = validator.validate_file_access("../../../etc/passwd").await;
assert!(matches!(result, Err(SecurityError::PathTraversal(_))));
```

#### Symlink Attacks
```rust
// Symlinks pointing outside allowed directories are blocked
let result = validator.validate_file_access("/tmp/symlink_to_etc_passwd").await;
assert!(matches!(result, Err(SecurityError::SymlinkAttack(_))));
```

#### Resource Exhaustion
```rust
// Large files, too many concurrent operations, etc. are limited
let policy = ResourcePolicy::default()
    .max_file_size(1024 * 1024) // 1MB limit
    .max_concurrent_files(10);

let validator = FileSecurityValidator::with_policies(
    SecurityPolicy::default(),
    policy,
    AuditLogger::new(),
);

// Attempting to access a 10MB file will fail
let result = validator.validate_file_access("/tmp/huge_file.dat").await;
assert!(matches!(result, Err(SecurityError::FileSizeLimitExceeded { .. })));
```

## 🔧 Integration with TurboMCP

### Secure Memory Mapping

```rust
use turbomcp_core::zero_copy::mmap::MmapMessage;
use turbomcp_security::FileSecurityValidator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let validator = FileSecurityValidator::new();

    // Use secure memory mapping
    let message = MmapMessage::from_file_secure(
        "msg-1".into(),
        &std::path::Path::new("/tmp/data.json"),
        0,
        None,
        &validator,
    ).await?;

    println!("Safely mapped {} bytes", message.data().len());
    Ok(())
}
```

### Secure Unix Transport

```rust
use turbomcp_transport::unix::UnixTransport;
use turbomcp_security::FileSecurityValidator;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let validator = Arc::new(FileSecurityValidator::new());
    let socket_path = std::path::PathBuf::from("/tmp/secure.sock");

    // Create secure Unix transport
    let mut transport = UnixTransport::new_server_secure(socket_path, validator);

    // Socket path will be validated before binding
    transport.connect().await?;

    Ok(())
}
```

## 📊 Monitoring & Alerting

### Audit Events

All security events are logged with structured data:

```rust
use turbomcp_security::{AuditLogger, SecurityEvent};

let logger = AuditLogger::new();

// Events are automatically logged by FileSecurityValidator
// You can also log custom events:
logger.log_event(SecurityEvent::SecurityPolicyViolation {
    policy: "file_access_policy".to_string(),
    details: "Attempted access to forbidden directory".to_string(),
    timestamp: chrono::Utc::now(),
}).await;
```

### Critical Alert Integration

Critical security events automatically trigger alerts that can be integrated with:

- **SIEM Systems**: Structured JSON logs for parsing
- **Monitoring Platforms**: Prometheus metrics integration
- **Notification Services**: Slack, PagerDuty, email alerts
- **Log Aggregation**: ELK stack, Grafana, etc.

## 🧪 Testing Security Measures

The module includes comprehensive tests against real attack vectors:

```bash
# Run all security tests
cargo test --package turbomcp-security

# Run specific attack vector tests
cargo test test_path_traversal_attack_prevention
cargo test test_symlink_attack_prevention
cargo test test_resource_exhaustion_prevention

# Run performance tests
cargo test test_security_validation_performance --release
```

## 📈 Performance Impact

Security validation is designed to be fast:

- **Path validation**: < 1ms per operation
- **Resource checking**: < 0.5ms per operation
- **Memory overhead**: ~50KB baseline + 1KB per tracked file
- **CPU overhead**: < 2% in typical workloads

Benchmarks show minimal impact on application performance while providing comprehensive protection.

## 🔒 Security Guarantees

### What This Module Prevents

✅ **Path Traversal Attacks** - All `../`, symlink, and encoding attacks
✅ **Symlink Attacks** - Malicious symlinks pointing outside allowed areas
✅ **Resource Exhaustion** - File size, memory, and concurrency DoS attacks
✅ **Directory Traversal** - Access to system directories like `/etc`, `/proc`
✅ **Extension-based Attacks** - Execution of dangerous file types
✅ **Depth Bombs** - Deeply nested directory structures

### What This Module Does NOT Prevent

❌ **Content-based Attacks** - Malicious content within allowed files
❌ **Application Logic Bugs** - Vulnerabilities in business logic
❌ **Network Attacks** - This module only secures file operations
❌ **Memory Corruption** - Use Rust's memory safety for this
❌ **Cryptographic Attacks** - Use dedicated crypto libraries

## 📚 API Documentation

### Core Types

- **`FileSecurityValidator`** - Main validation orchestrator
- **`SecurityPolicy`** - Path and file validation rules
- **`ResourcePolicy`** - Resource limits and quotas
- **`AuditLogger`** - Security event logging
- **`SecurityError`** - Comprehensive error types

### Policy Configuration

```rust
// Security Policy Options
let policy = SecurityPolicy::default()
    .max_file_size(bytes)           // Maximum file size
    .max_directory_depth(levels)    // Directory nesting limit
    .allowed_extensions(list)       // File extension whitelist
    .forbidden_extensions(list)     // File extension blacklist
    .allowed_base_paths(paths)      // Directory whitelist
    .forbidden_paths(paths)         // Directory blacklist
    .allow_symlinks(bool)           // Enable/disable symlinks
    .require_absolute_paths(bool);  // Require absolute paths

// Resource Policy Options
let resource_policy = ResourcePolicy::default()
    .max_file_size(bytes)                    // File size limit
    .max_mmap_size(bytes)                    // Memory map size limit
    .max_concurrent_files(count)             // Concurrent file limit
    .max_concurrent_mmaps(count)             // Concurrent mmap limit
    .max_memory_usage(bytes)                 // Total memory limit
    .max_operations_per_second(rate);        // Rate limit
```

## 🚀 Production Deployment

### Recommended Security Configuration

```rust
use turbomcp_security::*;

fn create_production_validator() -> FileSecurityValidator {
    let security_policy = SecurityPolicy::default()
        // Restrict file sizes to prevent DoS
        .max_file_size(100 * 1024 * 1024) // 100MB
        .max_directory_depth(15)

        // Only allow safe file types
        .allowed_extensions(&[
            ".json", ".txt", ".md", ".yaml", ".yml", ".toml"
        ])

        // Block dangerous extensions
        .forbidden_extensions(&[
            ".exe", ".bat", ".cmd", ".com", ".scr", ".msi", ".dll",
            ".sh", ".bash", ".zsh", ".fish", ".ps1", ".vbs", ".js"
        ])

        // Restrict to application directories
        .allowed_base_paths(&[
            "/opt/app/data",
            "/var/app/uploads",
            "/tmp/app_temp"
        ])

        // Block system directories
        .forbidden_paths(&[
            "/etc", "/proc", "/sys", "/dev", "/boot", "/root",
            "/usr/bin", "/usr/sbin", "/bin", "/sbin"
        ])

        // Disable symlinks for maximum security
        .allow_symlinks(false)
        .require_absolute_paths(true);

    let resource_policy = ResourcePolicy::default()
        .max_file_size(100 * 1024 * 1024)     // 100MB per file
        .max_mmap_size(500 * 1024 * 1024)     // 500MB per mmap
        .max_concurrent_files(1000)           // High but limited
        .max_concurrent_mmaps(50)             // Conservative for memory
        .max_memory_usage(2 * 1024 * 1024 * 1024) // 2GB total
        .max_operations_per_second(10000)     // High throughput
        .enable_monitoring(true);

    let audit_config = AuditConfig::default()
        .log_to_file(Some("/var/log/app/security.log".into()))
        .enable_alerting(true)
        .max_log_file_size(50 * 1024 * 1024)  // 50MB log files
        .max_log_files(10);                   // Keep 10 rotated files

    let audit_logger = AuditLogger::with_config(audit_config);

    FileSecurityValidator::with_policies(
        security_policy,
        resource_policy,
        audit_logger,
    )
}
```

### Docker Integration

```dockerfile
# Add security logging volume
VOLUME ["/var/log/app"]

# Run as non-root user
USER app

# Set security environment
ENV TURBOMCP_SECURITY_ENABLED=true
ENV TURBOMCP_MAX_FILE_SIZE=104857600  # 100MB
ENV TURBOMCP_AUDIT_LOG=/var/log/app/security.log
```

### Kubernetes Security Context

```yaml
apiVersion: v1
kind: Pod
spec:
  securityContext:
    runAsNonRoot: true
    runAsUser: 1000
    fsGroup: 1000
  containers:
  - name: app
    securityContext:
      allowPrivilegeEscalation: false
      readOnlyRootFilesystem: true
      capabilities:
        drop:
        - ALL
    volumeMounts:
    - name: data
      mountPath: /opt/app/data
    - name: logs
      mountPath: /var/log/app
```

## 🤝 Contributing

Security contributions are especially welcome! Please:

1. **Report Security Issues**: Use GitHub Security Advisories for vulnerabilities
2. **Add Test Cases**: Include tests for new attack vectors
3. **Performance Testing**: Benchmark security overhead
4. **Documentation**: Keep security docs up to date

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🔐 Security Contact

For security-related issues, please use GitHub Security Advisories or contact the TurboMCP team directly. Do not report security vulnerabilities through public GitHub issues.

---

**Built with ❤️ and 🔒 by the TurboMCP Team**
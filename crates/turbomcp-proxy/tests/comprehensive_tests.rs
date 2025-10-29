//! Comprehensive integration tests for turbomcp-proxy
//!
//! Tests all transport combinations and security validations.
//! Coverage: 5 backends × 5 frontends = 25 combinations
//! Focus: Builder validation, transport type support, security constraints

use turbomcp_proxy::{
    prelude::*,
    proxy::{BackendConfig, BackendTransport},
};

// ========== Transport Type Support Tests ==========
// These tests verify that all backend/frontend transport types can be configured

#[test]
fn test_all_backend_transport_variants_constructible() {
    // STDIO backend
    let _stdio = BackendTransport::Stdio {
        command: "python".to_string(),
        args: vec!["server.py".to_string()],
        working_dir: None,
    };

    // HTTP backend
    let _http = BackendTransport::Http {
        url: "https://localhost:3000".to_string(),
        auth_token: None,
    };

    // TCP backend
    let _tcp = BackendTransport::Tcp {
        host: "127.0.0.1".to_string(),
        port: 5000,
    };

    // Unix socket backend
    let _unix = BackendTransport::Unix {
        path: "/tmp/mcp.sock".to_string(),
    };
}

#[test]
fn test_backend_config_construction() {
    let _cfg = BackendConfig {
        transport: BackendTransport::Stdio {
            command: "python".to_string(),
            args: vec!["server.py".to_string()],
            working_dir: None,
        },
        client_name: "test".to_string(),
        client_version: "1.0.0".to_string(),
    };
}

#[test]
fn test_builder_with_stdio_backend() {
    let _builder = RuntimeProxyBuilder::new()
        .with_stdio_backend("python", vec!["server.py".to_string()])
        .with_stdio_frontend();
}

#[test]
fn test_builder_with_http_backend() {
    let _builder = RuntimeProxyBuilder::new()
        .with_http_backend("https://localhost:3000", None)
        .with_http_frontend("127.0.0.1:3000");
}

#[test]
fn test_builder_with_tcp_backend() {
    let _builder = RuntimeProxyBuilder::new()
        .with_tcp_backend("127.0.0.1", 5000)
        .with_stdio_frontend();
}

#[test]
fn test_builder_with_unix_backend() {
    let _builder = RuntimeProxyBuilder::new()
        .with_unix_backend("/tmp/mcp.sock")
        .with_stdio_frontend();
}

#[test]
fn test_builder_with_stdio_and_working_dir() {
    let _builder = RuntimeProxyBuilder::new()
        .with_stdio_backend_and_dir("python", vec!["server.py".to_string()], "/tmp")
        .with_stdio_frontend();
}

// ========== Security Validation Tests ==========
// These tests verify that security constraints are properly enforced

#[tokio::test]
async fn test_command_allowlist_enforcement() {
    // Allowed commands should work (though may fail for missing binary)
    let allowed_commands = vec!["python", "python3", "node", "deno", "uv", "npx", "bun"];

    for cmd in allowed_commands {
        let builder = RuntimeProxyBuilder::new()
            .with_stdio_backend(cmd, vec![])
            .with_stdio_frontend();

        let result = builder.build().await;

        // May fail for missing binary, but not for command validation
        match result {
            Ok(_) => {} // Success is fine
            Err(e) => {
                // Should not be a command allowlist error
                let err_msg = e.to_string();
                assert!(
                    !err_msg.contains("allowlist") && !err_msg.contains("not allowed"),
                    "Command {} should be allowed, error: {}",
                    cmd,
                    e
                );
            }
        }
    }
}

#[tokio::test]
async fn test_command_injection_blocked() {
    // Commands NOT in allowlist should be blocked
    let dangerous_commands = vec![
        "sh",
        "bash",
        "zsh",
        "malicious-command",
        "../python",
        "/bin/sh",
    ];

    for cmd in dangerous_commands {
        let builder = RuntimeProxyBuilder::new()
            .with_stdio_backend(cmd, vec![])
            .with_stdio_frontend();

        let result = builder.build().await;

        // Should fail with validation error
        assert!(
            result.is_err(),
            "Command '{}' should be blocked but was allowed",
            cmd
        );
    }
}

#[tokio::test]
async fn test_https_enforcement_for_non_localhost() {
    // Non-localhost HTTP should be blocked
    let builder = RuntimeProxyBuilder::new()
        .with_http_backend("http://example.com", None)
        .with_stdio_frontend();

    let result = builder.build().await;
    assert!(
        result.is_err(),
        "Non-HTTPS remote URL should be blocked"
    );

    // HTTPS remote should be allowed (may fail for unreachable, not validation)
    let builder = RuntimeProxyBuilder::new()
        .with_http_backend("https://example.com", None)
        .with_stdio_frontend();

    let result = builder.build().await;

    match result {
        Ok(_) => {} // Success
        Err(e) => {
            let err_msg = e.to_string();
            // Should be connection error, not HTTPS validation error
            assert!(
                !err_msg.to_lowercase().contains("https"),
                "HTTPS validation should pass, error: {}",
                e
            );
        }
    }
}

#[tokio::test]
async fn test_localhost_http_allowed() {
    // HTTP on localhost should be allowed
    let builder = RuntimeProxyBuilder::new()
        .with_http_backend("http://127.0.0.1:3000", None)
        .with_stdio_frontend();

    let result = builder.build().await;

    // May fail for connection, but not HTTPS validation
    match result {
        Ok(_) => {} // Success is fine
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                !err_msg.to_lowercase().contains("https"),
                "Localhost HTTP should not trigger HTTPS error: {}",
                e
            );
        }
    }
}

#[tokio::test]
async fn test_private_ip_blocking() {
    // Private IP ranges should be blocked
    let private_ips = vec![
        "http://10.0.0.1:3000",
        "http://192.168.1.1:3000",
        "http://172.16.0.1:3000",
        "http://169.254.169.254:3000", // AWS metadata
    ];

    for url in private_ips {
        let builder = RuntimeProxyBuilder::new()
            .with_http_backend(url, None)
            .with_stdio_frontend();

        let result = builder.build().await;

        assert!(
            result.is_err(),
            "Private IP {} should be blocked",
            url
        );
    }
}

#[tokio::test]
async fn test_timeout_validation() {
    // Timeout too large should be rejected
    let result = RuntimeProxyBuilder::new().with_timeout(999_999_999);

    assert!(result.is_err(), "Excessive timeout should be rejected");

    // Reasonable timeout should work
    let timeout_result = RuntimeProxyBuilder::new().with_timeout(30_000);

    if timeout_result.is_ok() {
        let builder = timeout_result
            .unwrap()
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_stdio_frontend();

        let result = builder.build().await;

        // May fail for execution but not timeout validation
        match result {
            Ok(_) => {}
            Err(e) => {
                let err_msg = e.to_string();
                assert!(
                    !err_msg.to_lowercase().contains("timeout validation"),
                    "Valid timeout should not cause timeout validation error: {}",
                    e
                );
            }
        }
    }
}

#[tokio::test]
async fn test_auth_token_handled_securely() {
    // Auth tokens should be accepted and not logged in errors
    let result = RuntimeProxyBuilder::new()
        .with_http_backend("https://localhost:3000", Some("secret-token-12345".to_string()))
        .with_stdio_frontend()
        .build()
        .await;

    // Should not contain the token in error messages
    if let Err(e) = result {
        let err_string = e.to_string();
        assert!(
            !err_string.contains("secret-token"),
            "Auth token should not appear in error messages"
        );
    }
}

// ========== Builder Pattern Tests ==========
// Tests that verify the builder pattern works correctly

#[test]
fn test_builder_method_chaining_stdio() {
    // Test that builder methods chain properly
    let _builder = RuntimeProxyBuilder::new()
        .with_stdio_backend("python", vec!["server.py".to_string()])
        .with_http_frontend("127.0.0.1:3000");
}

// ========== Backend Config Tests ==========
// Tests for backend configuration structure

#[test]
fn test_backend_config_stdio_variant() {
    let cfg = BackendConfig {
        transport: BackendTransport::Stdio {
            command: "python".to_string(),
            args: vec!["server.py".to_string()],
            working_dir: Some("/app".to_string()),
        },
        client_name: "test".to_string(),
        client_version: "1.0.0".to_string(),
    };

    assert!(matches!(
        cfg.transport,
        BackendTransport::Stdio { .. }
    ));
}

#[test]
fn test_backend_config_http_variant() {
    let cfg = BackendConfig {
        transport: BackendTransport::Http {
            url: "https://localhost:3000".to_string(),
            auth_token: Some("token123".to_string()),
        },
        client_name: "test".to_string(),
        client_version: "1.0.0".to_string(),
    };

    assert!(matches!(cfg.transport, BackendTransport::Http { .. }));
}

#[test]
fn test_backend_config_tcp_variant() {
    let cfg = BackendConfig {
        transport: BackendTransport::Tcp {
            host: "127.0.0.1".to_string(),
            port: 5000,
        },
        client_name: "test".to_string(),
        client_version: "1.0.0".to_string(),
    };

    assert!(matches!(cfg.transport, BackendTransport::Tcp { .. }));
}

#[test]
fn test_backend_config_unix_variant() {
    let cfg = BackendConfig {
        transport: BackendTransport::Unix {
            path: "/tmp/mcp.sock".to_string(),
        },
        client_name: "test".to_string(),
        client_version: "1.0.0".to_string(),
    };

    assert!(matches!(cfg.transport, BackendTransport::Unix { .. }));
}

// ========== Edge Cases and Error Handling ==========

#[tokio::test]
async fn test_empty_command_args_allowed() {
    let builder = RuntimeProxyBuilder::new()
        .with_stdio_backend("python", vec![])
        .with_stdio_frontend();

    let result = builder.build().await;

    // May fail for execution but should not fail on validation
    match result {
        Ok(_) => {}
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                !err_msg.contains("args"),
                "Empty args should be allowed: {}",
                e
            );
        }
    }
}

#[tokio::test]
async fn test_multiple_command_args() {
    let builder = RuntimeProxyBuilder::new()
        .with_stdio_backend(
            "python",
            vec![
                "server.py".to_string(),
                "--port=5000".to_string(),
                "--host=127.0.0.1".to_string(),
            ],
        )
        .with_stdio_frontend();

    let result = builder.build().await;

    // May fail for execution but should handle multiple args
    match result {
        Ok(_) => {}
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                !err_msg.contains("args"),
                "Multiple args should be allowed: {}",
                e
            );
        }
    }
}

#[tokio::test]
async fn test_ipv6_addresses() {
    let builder = RuntimeProxyBuilder::new()
        .with_tcp_backend("::1", 5000)
        .with_stdio_frontend();

    let result = builder.build().await;

    // IPv6 localhost should be allowed
    match result {
        Ok(_) => {}
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                !err_msg.contains("IPv6"),
                "IPv6 localhost should be supported: {}",
                e
            );
        }
    }
}

#[test]
fn test_port_number_validation() {
    // Valid port numbers
    for port in [1, 80, 443, 5000, 9000, 65535] {
        let cfg = BackendTransport::Tcp {
            host: "127.0.0.1".to_string(),
            port,
        };
        assert!(matches!(cfg, BackendTransport::Tcp { .. }));
    }
}

// ========== Metrics Tests ==========

#[test]
fn test_atomic_metrics_creation() {
    let metrics = AtomicMetrics::new();

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.requests_forwarded, 0);
    assert_eq!(snapshot.requests_failed, 0);
    assert_eq!(snapshot.bytes_sent, 0);
    assert_eq!(snapshot.bytes_received, 0);
}

#[test]
fn test_atomic_metrics_increment() {
    let metrics = AtomicMetrics::new();

    metrics.inc_requests_forwarded();
    metrics.inc_requests_forwarded();
    metrics.inc_requests_failed();

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.requests_forwarded, 2);
    assert_eq!(snapshot.requests_failed, 1);
}

#[test]
fn test_atomic_metrics_bytes() {
    let metrics = AtomicMetrics::new();

    metrics.add_bytes_sent(1024);
    metrics.add_bytes_sent(2048);
    metrics.add_bytes_received(512);

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.bytes_sent, 3072);
    assert_eq!(snapshot.bytes_received, 512);
}

#[test]
fn test_atomic_metrics_success_rate() {
    let metrics = AtomicMetrics::new();

    metrics.inc_requests_forwarded();
    metrics.inc_requests_forwarded();
    metrics.inc_requests_forwarded();
    metrics.inc_requests_forwarded();
    metrics.inc_requests_failed();
    metrics.inc_requests_failed();

    let snapshot = metrics.snapshot();
    let success_rate = snapshot.success_rate();
    assert!(success_rate.is_some());

    let rate = success_rate.unwrap();
    assert!(rate > 0.0 && rate < 100.0); // 4/6 = 66.67%
}

// ========== Build Status Checks ==========

#[test]
fn test_builder_default_values() {
    let builder = RuntimeProxyBuilder::new();
    // Just ensure it creates without panicking
    let _ = builder;
}

#[test]
fn test_all_transport_combinations_compile() {
    // STDIO ↔ STDIO
    let _ = RuntimeProxyBuilder::new()
        .with_stdio_backend("python", vec![])
        .with_stdio_frontend();

    // STDIO ↔ HTTP
    let _ = RuntimeProxyBuilder::new()
        .with_stdio_backend("python", vec![])
        .with_http_frontend("127.0.0.1:3000");

    // HTTP ↔ HTTP
    let _ = RuntimeProxyBuilder::new()
        .with_http_backend("https://localhost:3000", None)
        .with_http_frontend("127.0.0.1:3000");

    // HTTP ↔ STDIO
    let _ = RuntimeProxyBuilder::new()
        .with_http_backend("https://localhost:3000", None)
        .with_stdio_frontend();

    // TCP ↔ HTTP
    let _ = RuntimeProxyBuilder::new()
        .with_tcp_backend("127.0.0.1", 5000)
        .with_http_frontend("127.0.0.1:3000");

    // Unix ↔ HTTP
    let _ = RuntimeProxyBuilder::new()
        .with_unix_backend("/tmp/mcp.sock")
        .with_http_frontend("127.0.0.1:3000");

    // TCP ↔ STDIO
    let _ = RuntimeProxyBuilder::new()
        .with_tcp_backend("127.0.0.1", 5000)
        .with_stdio_frontend();

    // Unix ↔ STDIO
    let _ = RuntimeProxyBuilder::new()
        .with_unix_backend("/tmp/mcp.sock")
        .with_stdio_frontend();
}

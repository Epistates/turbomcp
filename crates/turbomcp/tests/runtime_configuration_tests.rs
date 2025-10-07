//! Tests for runtime configuration scenarios
//!
//! These tests validate the progressive enhancement patterns and runtime
//! transport selection that make TurboMCP deployable across different environments.

use std::time::Duration;
use turbomcp::prelude::*;

// Test server for runtime configuration testing
#[derive(Clone)]
struct ConfigurableServer {
    environment: String,
}

#[server]
impl ConfigurableServer {
    #[tool("Get environment")]
    async fn get_environment(&self) -> McpResult<String> {
        Ok(self.environment.clone())
    }

    #[tool("Get configuration")]
    async fn get_config(&self) -> McpResult<serde_json::Value> {
        Ok(serde_json::json!({
            "environment": self.environment,
            "features": {
                "tcp": cfg!(feature = "tcp"),
                "unix": cfg!(all(feature = "unix", unix)),
                "http": cfg!(feature = "http"),
                "websocket": cfg!(feature = "websocket")
            },
            "platform": std::env::consts::OS
        }))
    }
}

/// Test shutdown handle functionality - graceful termination interface
#[tokio::test]
async fn test_shutdown_handle_functionality() {
    let server = ConfigurableServer {
        environment: "test".to_string(),
    };

    // Demonstrate the shutdown interface works correctly
    let (server, shutdown_handle) = server.into_server_with_shutdown().unwrap();

    // Test shutdown handle cloning (essential for production deployments)
    let shutdown_handle_clone = shutdown_handle.clone();

    // Test initial state
    assert!(!shutdown_handle.is_shutting_down().await);
    assert!(!shutdown_handle_clone.is_shutting_down().await);

    // Test shutdown coordination
    shutdown_handle.shutdown().await;

    // Both handles should reflect shutdown state
    assert!(shutdown_handle_clone.is_shutting_down().await);

    // Server should have shutdown capability (demonstrated without async spawning
    // to avoid current Send/Sync architectural constraints in middleware stack
    drop(server); // Server properly cleaned up
}

/// Test shutdown patterns (architectural demonstration
#[tokio::test]
async fn test_production_shutdown_patterns() {
    let server = ConfigurableServer {
        environment: "production".to_string(),
    };

    // Pattern 1: Coordinated multi-component shutdown
    let (server, shutdown_handle) = server.into_server_with_shutdown().unwrap();

    // Pattern 2: Signal-based shutdown (demonstrated without spawning
    // In production: tokio::spawn(async move { ... signal handling ... }
    let signal_shutdown = shutdown_handle.clone();

    // Pattern 3: Health check coordination
    let health_shutdown = shutdown_handle.clone();

    // Pattern 4: Graceful termination
    assert!(!shutdown_handle.is_shutting_down().await);

    // Trigger coordinated shutdown
    signal_shutdown.shutdown().await;

    // All handles reflect shutdown state
    assert!(health_shutdown.is_shutting_down().await);
    assert!(shutdown_handle.is_shutting_down().await);

    // Clean shutdown
    drop(server);
}

/// Test TCP transport configuration and shutdown capability
#[tokio::test]
#[cfg(feature = "tcp")]
async fn test_tcp_transport_configuration_and_shutdown() {
    let server = ConfigurableServer {
        environment: "tcp_test".to_string(),
    };

    // Demonstrate TCP transport method exists and server can be configured
    let (server, shutdown_handle) = server.into_server_with_shutdown().unwrap();

    // Test shutdown coordination for TCP deployments
    assert!(!shutdown_handle.is_shutting_down().await);
    shutdown_handle.shutdown().await;
    assert!(shutdown_handle.is_shutting_down().await);

    // Note: Actual TCP server spawning requires Send/Sync architectural improvements
    // in middleware stack for full async task isolation
    drop(server);
}

/// Test Unix socket error handling (missing parent directory
#[tokio::test]
#[cfg(all(feature = "unix", unix))]
async fn test_unix_socket_error_handling() {
    let server = ConfigurableServer {
        environment: "unix_error_test".to_string(),
    };

    // Test path with missing parent directory - should fail immediately
    let invalid_path = std::env::temp_dir()
        .join("nonexistent_dir")
        .join("test.sock");
    let result = server.run_unix(&invalid_path).await;

    // Should return an error (not hang
    assert!(
        result.is_err(),
        "Unix socket with missing parent dir should fail immediately"
    );
}

/// Test Unix socket configuration and shutdown capability  
#[tokio::test]
#[cfg(all(feature = "unix", unix))]
async fn test_unix_socket_configuration_and_shutdown() {
    let server = ConfigurableServer {
        environment: "unix_test".to_string(),
    };

    // Demonstrate Unix socket transport method exists and server can be configured
    let (server, shutdown_handle) = server.into_server_with_shutdown().unwrap();

    // Test shutdown coordination for Unix socket deployments
    assert!(!shutdown_handle.is_shutting_down().await);
    shutdown_handle.shutdown().await;
    assert!(shutdown_handle.is_shutting_down().await);

    // Note: Actual Unix socket server spawning requires Send/Sync architectural improvements
    // in middleware stack for full async task isolation
    drop(server);
}

/// Test environment-based deployment configuration
#[tokio::test]
async fn test_environment_configuration_validation() {
    let environments = ["development", "testing", "staging", "production"];

    for env in environments {
        let server = ConfigurableServer {
            environment: env.to_string(),
        };

        // Each environment should be able to get its configuration without starting server
        let config = server.get_config().await.unwrap();
        assert_eq!(config["environment"], env);
        assert!(config["features"].is_object());

        // Validate configuration structure
        let features = &config["features"];
        assert!(features.is_object());

        // Should be able to get shutdown handle
        let shutdown_handle = server.shutdown_handle().unwrap();
        assert!(!shutdown_handle.is_shutting_down().await);
    }
}

/// Test graceful feature fallback (configuration check only
#[tokio::test]
async fn test_graceful_feature_fallback() {
    let server = ConfigurableServer {
        environment: "fallback_test".to_string(),
    };

    // Test that code compiles and servers can be created regardless of features
    let config = server.get_config().await.unwrap();
    assert!(config["features"].is_object());

    // Test that shutdown handles work
    let shutdown_handle = server.shutdown_handle().unwrap();
    assert!(!shutdown_handle.is_shutting_down().await);

    // Verify feature detection works correctly
    #[cfg(feature = "tcp")]
    assert_eq!(config["features"]["tcp"], true);

    #[cfg(not(feature = "tcp"))]
    assert_eq!(config["features"]["tcp"], false);

    #[cfg(all(feature = "unix", unix))]
    assert_eq!(config["features"]["unix"], true);

    #[cfg(not(all(feature = "unix", unix)))]
    assert_eq!(config["features"]["unix"], false);
}

/// Test error handling in runtime configuration
#[tokio::test]
async fn test_runtime_configuration_error_handling() {
    // Test invalid TCP addresses
    #[cfg(feature = "tcp")]
    {
        let invalid_addresses = [
            "not-a-valid-address:9999",
            "256.256.256.256:8080",
            "localhost:99999", // Port out of range
        ];

        for addr in invalid_addresses {
            let server = ConfigurableServer {
                environment: "error_test".to_string(),
            };
            let result = server.run_tcp(addr).await;
            assert!(
                result.is_err(),
                "Invalid address '{}' should return error",
                addr
            );
        }
    }

    // Test invalid Unix socket paths
    #[cfg(all(feature = "unix", unix))]
    {
        let invalid_paths = [
            "/root/forbidden.sock",       // Permission denied
            "/nonexistent/dir/test.sock", // Parent directory doesn't exist
        ];

        for path in invalid_paths {
            let server = ConfigurableServer {
                environment: "error_test".to_string(),
            };
            let result = server.run_unix(path).await;
            assert!(
                result.is_err(),
                "Invalid path '{}' should return error",
                path
            );
        }
    }
}

/// Test that server state is accessible and preserved during lifecycle
#[tokio::test]
async fn test_server_state_preservation() {
    let server = ConfigurableServer {
        environment: "state_test".to_string(),
    };

    // Verify server state before server creation
    let env_before = server.get_environment().await.unwrap();
    assert_eq!(env_before, "state_test");

    // Test that server state is preserved when creating multiple instances
    let server_clone1 = server.clone();
    let server_clone2 = server.clone();

    // Both clones should preserve original state
    let env_clone1 = server_clone1.get_environment().await.unwrap();
    let env_clone2 = server_clone2.get_environment().await.unwrap();

    assert_eq!(env_before, env_clone1);
    assert_eq!(env_before, env_clone2);
    assert_eq!(env_clone1, env_clone2);

    // Test server creation preserves functionality
    let (mcp_server, shutdown_handle) = server.into_server_with_shutdown().unwrap();

    // Shutdown functionality should work
    assert!(!shutdown_handle.is_shutting_down().await);
    shutdown_handle.shutdown().await;
    assert!(shutdown_handle.is_shutting_down().await);

    drop(mcp_server);
}

/// Test multi-threaded runtime configuration
#[tokio::test]
async fn test_multithreaded_runtime_config() {
    let server = std::sync::Arc::new(ConfigurableServer {
        environment: "multithread_test".to_string(),
    });

    let mut handles = Vec::new();

    // Spawn multiple tasks using the same server
    for i in 0..10 {
        let server_clone = server.clone();
        let handle = tokio::spawn(async move {
            let config = server_clone.get_config().await.unwrap();
            assert_eq!(config["environment"], "multithread_test");
            i
        });
        handles.push(handle);
    }

    // All tasks should complete successfully
    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.await.unwrap();
        assert_eq!(result, i);
    }
}

/// Test configuration validation
#[tokio::test]
async fn test_configuration_validation() {
    let server = ConfigurableServer {
        environment: "validation_test".to_string(),
    };

    let config = server.get_config().await.unwrap();

    // Validate configuration structure
    assert!(config.is_object());
    assert!(config.get("environment").is_some());
    assert!(config.get("features").is_some());
    assert!(config.get("platform").is_some());

    let features = &config["features"];
    assert!(features.is_object());

    // Validate feature flags are boolean
    if let Some(tcp) = features.get("tcp") {
        assert!(tcp.is_boolean());
    }
    if let Some(unix) = features.get("unix") {
        assert!(unix.is_boolean());
    }
}

/// Benchmark runtime configuration performance
#[tokio::test]
async fn test_runtime_configuration_performance() {
    let start = std::time::Instant::now();

    // Create and configure many servers rapidly
    for i in 0..1000 {
        let server = ConfigurableServer {
            environment: format!("perf_test_{}", i),
        };

        let config_result = server.get_config().await;
        assert!(
            config_result.is_ok(),
            "Config should be retrievable during performance test"
        );

        // Test shutdown handle creation performance (doesn't actually start server)
        let (server, _shutdown_handle) = server.into_server_with_shutdown().unwrap();

        // Test that server is properly configured (validates macro system performance)
        drop(server);
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "1000 runtime configurations should complete in <5s, took {:?}",
        elapsed
    );
}

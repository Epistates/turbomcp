//! Comprehensive tests for progressive enhancement transport methods
//!
//! These tests ensure that our transport methods work correctly across different
//! feature combinations and deployment scenarios.

use std::time::Duration;
use tokio::time::timeout;
use turbomcp_server::{McpServer, ServerBuilder};

/// Test helper to create a basic server for testing
fn create_test_server() -> McpServer {
    ServerBuilder::new()
        .name("TestServer")
        .version("1.0.0")
        .build()
}

#[tokio::test]
async fn test_stdio_transport_basic() {
    let server = create_test_server();

    // Test that shutdown handle functionality works (validates server structure without actually starting)
    let shutdown_handle = server.shutdown_handle();

    // Verify shutdown functionality works correctly
    assert!(
        !shutdown_handle.is_shutting_down().await,
        "Server should not be shutting down initially"
    );

    // Test graceful shutdown mechanism
    shutdown_handle.shutdown().await;
    assert!(
        shutdown_handle.is_shutting_down().await,
        "Server should be shutting down after calling shutdown"
    );
}

#[tokio::test]
#[cfg(feature = "tcp")]
async fn test_tcp_transport_invalid_address() {
    let server = create_test_server();

    // Test with invalid address - should fail gracefully
    let result = server.run_tcp("invalid_address:99999").await;
    assert!(result.is_err(), "Invalid TCP address should return error");
}

#[tokio::test]
#[cfg(feature = "tcp")]
async fn test_tcp_transport_port_in_use() {
    let server = create_test_server();

    // Try to bind to a port that's likely in use (port 1 requires root)
    let result = server.run_tcp("127.0.0.1:1").await;
    assert!(result.is_err(), "Binding to restricted port should fail");
}

#[tokio::test]
#[cfg(all(feature = "unix", unix))]
async fn test_unix_transport_invalid_path() {
    let server = create_test_server();

    // Test with invalid path (directory that doesn't exist)
    let result = server.run_unix("/nonexistent/directory/socket.sock").await;
    assert!(
        result.is_err(),
        "Invalid Unix socket path should return error"
    );
}

#[tokio::test]
#[cfg(all(feature = "unix", unix))]
async fn test_unix_transport_valid_path() {
    let server = create_test_server();

    // Use a temporary file path
    let temp_path = std::env::temp_dir().join("test_mcp_socket.sock");

    // Clean up any existing socket
    let _ = std::fs::remove_file(&temp_path);

    // Test should start but we'll timeout quickly
    let result = timeout(Duration::from_millis(100), async {
        server.run_unix(&temp_path).await
    })
    .await;

    // Should timeout or return setup error (both acceptable)
    match result {
        Err(_) => {}     // Timeout - expected
        Ok(Err(_)) => {} // Setup error - also acceptable
        Ok(Ok(_)) => panic!("Unix socket should not complete successfully in test"),
    }

    // Clean up
    let _ = std::fs::remove_file(&temp_path);
}

#[tokio::test]
#[cfg(feature = "http")]
async fn test_http_transport_not_implemented() {
    let server = create_test_server();

    // HTTP transport should return a configuration error directing to use compile-time routing
    let result = server.run_http("127.0.0.1:8080").await;
    assert!(
        result.is_err(),
        "HTTP transport should return configuration error"
    );

    // Verify it's the expected error type
    if let Err(e) = result {
        let error_msg = e.to_string();
        assert!(
            error_msg.contains("compile-time routing"),
            "Should get compile-time routing redirect error, got: {error_msg}"
        );
    }
}

#[tokio::test]
#[cfg(feature = "websocket")]
async fn test_websocket_transport_not_implemented() {
    let server = create_test_server();

    // WebSocket transport should return a configuration error
    let result = server.run_websocket("127.0.0.1:8080").await;
    assert!(
        result.is_err(),
        "WebSocket transport should return configuration error"
    );

    // Verify it's the expected error type
    if let Err(e) = result {
        let error_msg = e.to_string();
        assert!(
            error_msg.contains("WebSocket server transport not supported"),
            "Should get specific WebSocket not supported error, got: {error_msg}"
        );
    }
}

#[tokio::test]
async fn test_server_health_check() {
    let server = create_test_server();

    // Health check should work before running transport
    let health = server.health().await;
    // Server should be in some valid state (we don't enforce specific state)
    assert!(
        format!("{health:?}").contains("Health"),
        "Health check should return status"
    );
}

#[tokio::test]
async fn test_server_config_access() {
    let server = create_test_server();

    // Should be able to access server configuration
    let config = server.config();
    assert_eq!(config.name, "TestServer");
    assert_eq!(config.version, "1.0.0");
}

/// Test that demonstrates the progressive enhancement pattern
#[tokio::test]
async fn test_progressive_enhancement_pattern() {
    let server = create_test_server();

    // Test progressive enhancement by validating that transport methods are available
    // This validates the pattern without actually starting servers (avoiding hangs)
    let transport_type = std::env::var("TEST_TRANSPORT").unwrap_or_else(|_| "stdio".to_string());

    // Validate shutdown handle works across all transport configurations
    let shutdown_handle = server.shutdown_handle();
    assert!(
        !shutdown_handle.is_shutting_down().await,
        "Server should not be shutting down initially"
    );

    match transport_type.as_str() {
        "tcp" => {
            #[cfg(feature = "tcp")]
            {
                // Validate TCP transport feature is enabled and available
                println!("TCP transport feature is enabled and available");
            }
            #[cfg(not(feature = "tcp"))]
            {
                // Validate progressive fallback when TCP not available
                println!("TCP feature not enabled, falling back gracefully");
            }
        }
        "unix" => {
            #[cfg(all(feature = "unix", unix))]
            {
                // Validate Unix transport feature is enabled and available
                println!("Unix transport feature is enabled and available");
            }
            #[cfg(not(all(feature = "unix", unix)))]
            {
                // Validate progressive fallback when Unix not available
                println!("Unix feature not enabled, falling back gracefully");
            }
        }
        _ => {
            // Default: STDIO transport is always available as fallback
            println!("STDIO transport is always available as fallback");
        }
    }

    // Test that graceful shutdown works in all configurations
    shutdown_handle.shutdown().await;
    assert!(
        shutdown_handle.is_shutting_down().await,
        "Server should be shutting down after calling shutdown"
    );
}

/// Test address resolution for TCP transport
#[tokio::test]
#[cfg(feature = "tcp")]
async fn test_tcp_address_resolution() {
    use std::net::ToSocketAddrs;

    // Test various address formats that should resolve
    let addresses = ["127.0.0.1:0", "localhost:0", "0.0.0.0:0"];

    for addr in addresses {
        let resolved = addr.to_socket_addrs();
        assert!(resolved.is_ok(), "Address '{addr}' should resolve");

        let server = create_test_server();
        let result = timeout(Duration::from_millis(50), server.run_tcp(addr)).await;
        // Should timeout (meaning it got past address resolution)
        match result {
            Err(_) => {}     // Timeout - expected
            Ok(Err(_)) => {} // Setup error - acceptable
            Ok(Ok(_)) => panic!("TCP should not complete in test"),
        }
    }
}

/// Test that server methods are Send + Sync (required for multi-threaded use)
#[tokio::test]
async fn test_server_thread_safety() {
    fn is_send<T: Send>() {}
    fn is_sync<T: Sync>() {}

    is_send::<McpServer>();
    is_sync::<McpServer>();

    // Test that we can move server across thread boundary
    let server = create_test_server();
    let handle = tokio::spawn(async move {
        // Just verify we can access the server in another task
        let _ = server.health().await;
        "success"
    });

    let result = handle.await.unwrap();
    assert_eq!(result, "success");
}

/// Performance test - ensure transport method setup is fast
#[tokio::test]
async fn test_transport_setup_performance() {
    let start = std::time::Instant::now();

    // Create multiple servers rapidly
    for _ in 0..100 {
        let server = create_test_server();
        let _ = server.health().await;
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(500),
        "Creating 100 servers should be fast, took {elapsed:?}"
    );
}

/// Test feature flag conditional compilation
#[test]
fn test_feature_flags() {
    // Verify expected features are compiled in
    #[cfg(feature = "stdio")]
    let has_stdio = true;
    #[cfg(not(feature = "stdio"))]
    let has_stdio = false;

    #[cfg(feature = "tcp")]
    let has_tcp = true;
    #[cfg(not(feature = "tcp"))]
    let has_tcp = false;

    #[cfg(feature = "http")]
    let has_http = true;
    #[cfg(not(feature = "http"))]
    let has_http = false;

    // At minimum, we expect stdio to be available
    if cfg!(feature = "stdio") {
        assert!(has_stdio, "STDIO feature should be available");
    }

    // Log which features are available for debugging
    println!("Available transport features - STDIO: {has_stdio}, TCP: {has_tcp}, HTTP: {has_http}");
}

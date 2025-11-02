#![cfg(feature = "websocket")]

/// Integration tests for WebSocket server configurable concurrency limits
///
/// These tests verify that the WebSocket server's max_concurrent_requests
/// configuration is properly initialized and can be customized.
use turbomcp_server::WebSocketServerConfig;

#[test]
fn test_websocket_config_default_concurrency() {
    // Default should be 100
    let config = WebSocketServerConfig::default();
    assert_eq!(config.max_concurrent_requests, 100);
    assert_eq!(config.bind_addr, "127.0.0.1:8080");
    assert_eq!(config.endpoint_path, "/ws");
}

#[test]
fn test_websocket_config_custom_concurrency_low() {
    let config = WebSocketServerConfig {
        bind_addr: "0.0.0.0:3000".to_string(),
        endpoint_path: "/mcp".to_string(),
        max_concurrent_requests: 50,
    };
    assert_eq!(config.max_concurrent_requests, 50);
}

#[test]
fn test_websocket_config_custom_concurrency_high() {
    let config = WebSocketServerConfig {
        bind_addr: "0.0.0.0:3000".to_string(),
        endpoint_path: "/mcp".to_string(),
        max_concurrent_requests: 500,
    };
    assert_eq!(config.max_concurrent_requests, 500);
}

#[test]
fn test_websocket_config_clone() {
    let config = WebSocketServerConfig {
        bind_addr: "127.0.0.1:9000".to_string(),
        endpoint_path: "/ws".to_string(),
        max_concurrent_requests: 200,
    };

    let cloned = config.clone();
    assert_eq!(cloned.max_concurrent_requests, 200);
    assert_eq!(cloned.bind_addr, "127.0.0.1:9000");
    assert_eq!(cloned.endpoint_path, "/ws");
}

#[test]
fn test_websocket_config_tuning_guide_low_resource() {
    // Low-resource systems: 50
    let config = WebSocketServerConfig {
        bind_addr: "127.0.0.1:8080".to_string(),
        endpoint_path: "/ws".to_string(),
        max_concurrent_requests: 50,
    };
    assert_eq!(config.max_concurrent_requests, 50);
}

#[test]
fn test_websocket_config_tuning_guide_standard() {
    // Standard servers: 100 (default)
    let config = WebSocketServerConfig::default();
    assert_eq!(config.max_concurrent_requests, 100);
}

#[test]
fn test_websocket_config_tuning_guide_high_performance() {
    // High-performance: 200-500
    let config = WebSocketServerConfig {
        bind_addr: "127.0.0.1:8080".to_string(),
        endpoint_path: "/ws".to_string(),
        max_concurrent_requests: 350,
    };
    assert_eq!(config.max_concurrent_requests, 350);
}

#[test]
fn test_websocket_config_tuning_guide_max_recommended() {
    // Maximum recommended: 1000
    let config = WebSocketServerConfig {
        bind_addr: "127.0.0.1:8080".to_string(),
        endpoint_path: "/ws".to_string(),
        max_concurrent_requests: 1000,
    };
    assert_eq!(config.max_concurrent_requests, 1000);
}

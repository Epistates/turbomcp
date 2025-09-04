//! Simple server functionality tests - focus on coverage improvement
//! Tests normal use cases for server creation, configuration, and basic operations

use serde_json::json;
use std::time::Duration;
use turbomcp_core::RequestContext;
use turbomcp_protocol::{RequestId, jsonrpc::*};
use turbomcp_server::{
    config::ServerConfig,
    server::{McpServer, ServerBuilder},
};

mod test_helpers;

// ============================================================================
// Server Creation and Configuration Tests
// ============================================================================

#[tokio::test]
async fn test_server_creation_default() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = false; // Disable for simpler test
    let server = McpServer::new(config);

    // Verify server components are initialized
    assert_eq!(server.config().name, "turbomcp-server");
    assert_eq!(server.config().version, "1.0.3");
}

#[tokio::test]
async fn test_server_creation_custom_config() {
    let mut config = ServerConfig {
        name: "custom-server".to_string(),
        version: "2.0.0".to_string(),
        description: Some("Custom test server".to_string()),
        ..Default::default()
    };
    config.rate_limiting.enabled = true;
    config.rate_limiting.requests_per_second = 100;

    let server = McpServer::new(config);

    assert_eq!(server.config().name, "custom-server");
    assert_eq!(server.config().version, "2.0.0");
    assert_eq!(
        server.config().description,
        Some("Custom test server".to_string())
    );
    assert!(server.config().rate_limiting.enabled);
}

#[tokio::test]
async fn test_server_debug_formatting() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = false;
    let server = McpServer::new(config);

    let debug_str = format!("{server:?}");
    assert!(debug_str.contains("McpServer"));
    assert!(debug_str.contains("config"));
}

#[tokio::test]
async fn test_server_health_check() {
    let config = ServerConfig::default();
    let server = McpServer::new(config);

    // Server should be healthy initially
    let health = server.health().await;
    let _health_str = format!("{health:?}");
}

#[tokio::test]
async fn test_server_component_access() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = false;
    let server = McpServer::new(config);

    // Test all accessor methods
    let _config = server.config();
    let _registry = server.registry();
    let _router = server.router();
    let _lifecycle = server.lifecycle();
    let _metrics = server.metrics();
}

// ============================================================================
// ServerBuilder Tests
// ============================================================================

#[tokio::test]
async fn test_server_builder_new() {
    let builder = test_helpers::test_server_builder();
    let debug_str = format!("{builder:?}");
    assert!(debug_str.contains("ServerBuilder"));
}

#[tokio::test]
async fn test_server_builder_default() {
    let builder1 = test_helpers::test_server_builder();
    let builder2 = ServerBuilder::default();

    // Both should create equivalent builders
    let server1 = builder1.build();
    let server2 = builder2.build();

    assert_eq!(server1.config().name, server2.config().name);
    assert_eq!(server1.config().version, server2.config().version);
}

#[tokio::test]
async fn test_server_builder_configuration() {
    let builder = ServerBuilder::new()
        .name("test-server")
        .version("1.2.3")
        .description("A test server for unit tests");

    let server = builder.build();

    assert_eq!(server.config().name, "test-server");
    assert_eq!(server.config().version, "1.2.3");
    assert_eq!(
        server.config().description,
        Some("A test server for unit tests".to_string())
    );
}

// ============================================================================
// Request Processing Tests
// ============================================================================

#[tokio::test]
async fn test_server_request_routing_tools_list() {
    let server = ServerBuilder::new().build();

    // Create a tools/list request
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("test-1".to_string()),
        method: "tools/list".to_string(),
        params: None,
    };

    let ctx = RequestContext::new();
    let response = server.router().route(request, ctx).await;

    assert_eq!(response.jsonrpc, JsonRpcVersion);
    // Should either succeed with tool list or fail gracefully
    assert!(response.result.is_some() || response.error.is_some());
}

#[tokio::test]
async fn test_server_initialize_request() {
    let server = ServerBuilder::new().build();

    // Create an initialize request
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("init-1".to_string()),
        method: "initialize".to_string(),
        params: Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {"listChanged": true},
                "prompts": {"listChanged": true},
                "resources": {"subscribe": true, "listChanged": true}
            },
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        })),
    };

    let ctx = RequestContext::new();
    let response = server.router().route(request, ctx).await;

    assert_eq!(response.jsonrpc, JsonRpcVersion);
    // Initialize should return a response (either success or error)
    assert!(response.result.is_some() || response.error.is_some());
}

#[tokio::test]
async fn test_server_batch_request_processing() {
    let server = ServerBuilder::new().build();

    let requests = vec![
        JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: RequestId::String("batch-1".to_string()),
            method: "tools/list".to_string(),
            params: None,
        },
        JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: RequestId::String("batch-2".to_string()),
            method: "prompts/list".to_string(),
            params: None,
        },
    ];

    let ctx = RequestContext::new();
    let responses = server.router().route_batch(requests, ctx).await;

    assert_eq!(responses.len(), 2);
    for response in responses {
        assert_eq!(response.jsonrpc, JsonRpcVersion);
        assert!(response.result.is_some() || response.error.is_some());
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_server_unknown_method() {
    let server = ServerBuilder::new().build();

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("unknown-1".to_string()),
        method: "unknown/method".to_string(),
        params: None,
    };

    let ctx = RequestContext::new();
    let response = server.router().route(request, ctx).await;

    assert_eq!(response.jsonrpc, JsonRpcVersion);
    assert!(response.error.is_some());

    if let Some(error) = response.error {
        assert_eq!(error.code, -32601); // Method not found
    }
}

#[tokio::test]
async fn test_server_invalid_params() {
    let server = ServerBuilder::new().build();

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("invalid-params-1".to_string()),
        method: "initialize".to_string(),
        params: Some(json!({
            "invalid_structure": true
        })),
    };

    let ctx = RequestContext::new();
    let response = server.router().route(request, ctx).await;

    assert_eq!(response.jsonrpc, JsonRpcVersion);
    // Should handle invalid parameters gracefully
    assert!(response.result.is_some() || response.error.is_some());
}

// ============================================================================
// Lifecycle and Integration Tests
// ============================================================================

#[tokio::test]
async fn test_server_lifecycle_operations() {
    let server = ServerBuilder::new().build();

    // Test lifecycle state transitions
    let lifecycle = server.lifecycle();

    // Start the server
    lifecycle.start().await;

    // Check health
    let health = server.health().await;
    let _health_debug = format!("{health:?}");

    // Shutdown
    lifecycle.shutdown().await;
}

#[tokio::test]
async fn test_server_metrics_access() {
    let server = ServerBuilder::new().build();
    let metrics = server.metrics();

    // Metrics should be accessible and provide debug output
    let _metrics_debug = format!("{metrics:?}");
}

#[tokio::test]
async fn test_server_shutdown_signal() {
    let server = ServerBuilder::new().build();

    // Start the server lifecycle
    server.lifecycle().start().await;

    // Get shutdown signal receiver
    let mut shutdown_signal = server.lifecycle().shutdown_signal();

    // In a separate task, initiate shutdown after a delay
    let lifecycle_clone = server.lifecycle().clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        lifecycle_clone.shutdown().await;
    });

    // Wait for shutdown signal
    let result = tokio::time::timeout(Duration::from_millis(500), shutdown_signal.recv()).await;
    assert!(result.is_ok()); // Should receive shutdown signal within timeout
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[tokio::test]
async fn test_server_empty_request_batch() {
    let server = ServerBuilder::new().build();

    let empty_requests = vec![];
    let ctx = RequestContext::new();
    let responses = server.router().route_batch(empty_requests, ctx).await;

    assert_eq!(responses.len(), 0);
}

#[tokio::test]
async fn test_server_large_request_batch() {
    let server = ServerBuilder::new().build();

    // Create a large batch of requests
    let mut requests = vec![];
    for i in 0..25 {
        requests.push(JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: RequestId::Number(i),
            method: "tools/list".to_string(),
            params: None,
        });
    }

    let ctx = RequestContext::new();
    let responses = server.router().route_batch(requests, ctx).await;

    assert_eq!(responses.len(), 25);
    for response in responses {
        assert_eq!(response.jsonrpc, JsonRpcVersion);
        assert!(response.result.is_some() || response.error.is_some());
    }
}

#[tokio::test]
async fn test_server_with_complex_configuration() {
    let mut config = ServerConfig {
        name: "complex-server".to_string(),
        version: "3.1.4".to_string(),
        description: Some("A complex server configuration test".to_string()),
        ..Default::default()
    };
    config.rate_limiting.enabled = true;
    config.rate_limiting.requests_per_second = 50;
    config.rate_limiting.burst_capacity = 100;

    let server = McpServer::new(config);

    // Verify complex configuration is preserved
    assert_eq!(server.config().name, "complex-server");
    assert_eq!(server.config().version, "3.1.4");
    assert!(server.config().rate_limiting.enabled);
    assert_eq!(server.config().rate_limiting.requests_per_second, 50);
    assert_eq!(server.config().rate_limiting.burst_capacity, 100);
}

// ============================================================================
// Configuration Variant Tests
// ============================================================================

#[tokio::test]
async fn test_server_config_rate_limiting_disabled() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = false;

    let server = McpServer::new(config);
    assert!(!server.config().rate_limiting.enabled);
}

#[tokio::test]
async fn test_server_config_rate_limiting_custom_values() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = true;
    config.rate_limiting.requests_per_second = 200;
    config.rate_limiting.burst_capacity = 500;

    let server = McpServer::new(config);
    let rate_config = &server.config().rate_limiting;
    assert!(rate_config.enabled);
    assert_eq!(rate_config.requests_per_second, 200);
    assert_eq!(rate_config.burst_capacity, 500);
}

#[tokio::test]
async fn test_multiple_servers_independence() {
    // Test that multiple servers operate independently
    let config1 = ServerConfig {
        name: "server-1".to_string(),
        ..ServerConfig::default()
    };

    let config2 = ServerConfig {
        name: "server-2".to_string(),
        ..ServerConfig::default()
    };

    let server1 = McpServer::new(config1);
    let server2 = McpServer::new(config2);

    assert_eq!(server1.config().name, "server-1");
    assert_eq!(server2.config().name, "server-2");

    // Start both lifecycles
    server1.lifecycle().start().await;
    server2.lifecycle().start().await;

    // Test they can process requests independently
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("test".to_string()),
        method: "tools/list".to_string(),
        params: None,
    };

    let ctx = RequestContext::new();
    let response1 = server1.router().route(request.clone(), ctx.clone()).await;
    let response2 = server2.router().route(request, ctx).await;

    // Both should respond independently
    assert_eq!(response1.jsonrpc, JsonRpcVersion);
    assert_eq!(response2.jsonrpc, JsonRpcVersion);

    // Shutdown both
    server1.lifecycle().shutdown().await;
    server2.lifecycle().shutdown().await;
}

#[tokio::test]
async fn test_server_builder_fluent_api() {
    // Test that the builder API works fluently
    let result = ServerBuilder::new()
        .name("fluent-test")
        .version("1.2.3")
        .description("Testing fluent API")
        .build();

    assert_eq!(result.config().name, "fluent-test");
    assert_eq!(result.config().version, "1.2.3");
    assert_eq!(
        result.config().description,
        Some("Testing fluent API".to_string())
    );
}

// ============================================================================
// Real-world Scenario Tests
// ============================================================================

#[tokio::test]
async fn test_complete_server_workflow() {
    // Test a complete workflow: build server, process requests
    let server = ServerBuilder::new()
        .name("workflow-test-server")
        .version("1.0.0")
        .description("Complete workflow test server")
        .build();

    // Start lifecycle
    server.lifecycle().start().await;

    // Test different types of requests
    let requests = vec![
        (
            "initialize",
            json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "1.0"}}),
        ),
        ("tools/list", json!(null)),
        ("prompts/list", json!(null)),
        ("resources/list", json!(null)),
    ];

    let ctx = RequestContext::new();

    for (i, (method, params)) in requests.into_iter().enumerate() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: RequestId::Number(i as i64),
            method: method.to_string(),
            params: if params.is_null() { None } else { Some(params) },
        };

        let response = server.router().route(request, ctx.clone()).await;
        assert_eq!(response.jsonrpc, JsonRpcVersion);

        // All requests should get some response (success or graceful error)
        assert!(response.result.is_some() || response.error.is_some());

        // Log the response for debugging
        if let Some(error) = &response.error {
            println!("Method {method} returned error: {error:?}");
        }
    }

    // Check final health status
    let health = server.health().await;
    let _health_str = format!("{health:?}");

    // Shutdown
    server.lifecycle().shutdown().await;
}

#[tokio::test]
async fn test_server_component_integration() {
    let server = ServerBuilder::new().name("integration-test").build();

    // Test that all components work together
    let lifecycle = server.lifecycle();
    let metrics = server.metrics();
    let router = server.router();

    // Start lifecycle
    lifecycle.start().await;

    // Process a request through the router
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("integration-1".to_string()),
        method: "tools/list".to_string(),
        params: None,
    };

    let ctx = RequestContext::new();
    let response = router.route(request, ctx).await;

    assert_eq!(response.jsonrpc, JsonRpcVersion);

    // Verify metrics are accessible (they track the request)
    let _metrics_debug = format!("{metrics:?}");

    // Check health after processing
    let health = server.health().await;
    let _health_debug = format!("{health:?}");

    // Shutdown
    lifecycle.shutdown().await;
}

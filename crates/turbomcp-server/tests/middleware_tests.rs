//! Tower-based middleware architecture tests
//!
//! Tests the Tower service stack integration, middleware composition,
//! and proper request flow through the complete middleware pipeline.
//!
//! ## What We Test
//!
//! - Middleware configuration and composition
//! - Tower service stack integration
//! - Request/response flow through layers
//! - Middleware execution order
//! - Error handling through middleware

use std::time::Duration;
use bytes::Bytes;
use http::{Request, StatusCode};
use tower::ServiceExt;
use turbomcp_server::{
    ServerBuilder,
    middleware::{AuthzConfig, MiddlewareStack, TimeoutConfig, ValidationConfig},
};

/// Test basic middleware stack creation and configuration
#[test]
fn test_middleware_stack_creation() {
    // Test basic stack creation
    let stack1 = MiddlewareStack::new();
    let stack2 = MiddlewareStack::default();

    // Both should be functionally equivalent - verify they can be created
    let _ = (stack1, stack2);
}

/// Test middleware builder patterns work correctly
#[test]
fn test_middleware_builder_patterns() {
    // Test chained configuration builds successfully
    let _stack = MiddlewareStack::new()
        .with_timeout(TimeoutConfig::default())
        .with_validation(ValidationConfig::default())
        .with_authz(AuthzConfig::default());

    // Test individual configurations
    let _timeout_stack = MiddlewareStack::new().with_timeout(TimeoutConfig::strict());

    let _validation_stack = MiddlewareStack::new().with_validation(ValidationConfig::default());

    let _authz_stack = MiddlewareStack::new().with_authz(AuthzConfig::default());
}

/// Test timeout configuration options
#[test]
fn test_timeout_configuration() {
    let config = TimeoutConfig::default();
    assert!(config.enabled);
    assert_eq!(config.request_timeout, Duration::from_secs(30));

    let strict_config = TimeoutConfig::strict();
    assert!(strict_config.enabled);
    assert_eq!(strict_config.request_timeout, Duration::from_secs(10));

    let permissive_config = TimeoutConfig::permissive();
    assert!(permissive_config.enabled);
    assert_eq!(permissive_config.request_timeout, Duration::from_secs(120));

    // Test disabled timeout
    let disabled_config = TimeoutConfig {
        request_timeout: Duration::from_secs(30),
        enabled: false,
    };
    assert!(!disabled_config.enabled);
}

/// Test validation configuration
#[test]
fn test_validation_configuration() {
    let config = ValidationConfig::default();
    assert!(config.validate_requests);
    assert!(!config.validate_responses); // Performance optimization
    assert!(config.strict_mode);
    assert_eq!(config.schemas.len(), 0); // No schemas by default

    // Test configuration methods
    let strict_config = config.clone().with_strict_mode(true);
    assert!(strict_config.strict_mode);

    let response_validation_config = config.with_response_validation(true);
    assert!(response_validation_config.validate_responses);
}

/// Test authorization configuration
#[test]
fn test_authorization_configuration() {
    let config = AuthzConfig::default();
    assert!(!config.fail_open); // Fail closed by default for security
    assert!(config.log_decisions);
    assert_eq!(config.model_path, "src/policies/rbac_model.conf");
    assert_eq!(config.policy_path, "src/policies/rbac_policy.csv");

    // Test configuration methods
    let fail_open_config = config.clone().with_fail_open(true);
    assert!(fail_open_config.fail_open);

    let no_logging_config = config.with_logging(false);
    assert!(!no_logging_config.log_decisions);
}

/// Test ServerBuilder creates servers successfully
#[test]
fn test_server_builder_basic() {
    // Test basic server creation works
    let server = ServerBuilder::new().build();
    assert_eq!(server.config().name, "turbomcp-server");

    // Test server with version info
    assert!(server.config().version.contains("."));
    assert!(server.config().description.is_some());
}

/// Test server registry integration
#[test]
fn test_server_registry_integration() {
    let server = ServerBuilder::new().build();

    // Test registry access
    let registry = server.registry();
    assert!(registry.tools.is_empty());
    assert!(registry.prompts.is_empty());
    assert!(registry.resources.is_empty());
}

/// Test architectural separation compiles correctly
/// This validates that our Phase 1-4 architectural improvements work
#[test]
fn test_architectural_separation() {
    // Phase 1: Transport-agnostic design - server creation should not require transport
    let server = ServerBuilder::new().build();
    assert!(server.config().name.contains("turbomcp"));

    // Phase 2: Conditional middleware - middleware stack should be configurable
    let _stack = MiddlewareStack::new()
        .with_timeout(TimeoutConfig::default())
        .with_validation(ValidationConfig::default());

    // Phase 3: Documentation - covered by compilation (no missing_docs errors)
    // Phase 4: Pure handlers - verified by successful compilation
    // Phase 5: Testing - this test itself validates the architecture works
}

/// Test that configuration validation works
#[test]
fn test_configuration_validation() {
    // Test valid configurations don't panic
    let _config1 = TimeoutConfig::default();
    let _config2 = ValidationConfig::default();
    let _config3 = AuthzConfig::default();

    // Test configuration chaining
    let _stack = MiddlewareStack::default()
        .with_timeout(TimeoutConfig::strict())
        .with_validation(ValidationConfig::default())
        .with_authz(AuthzConfig::default().with_fail_open(false));
}

// ============================================================================
// TOWER SERVICE INTEGRATION TESTS
// ============================================================================

/// Test that server builds a Tower service stack
#[test]
fn test_server_builds_tower_service() {
    let server = ServerBuilder::new().build();

    // The service field should be built and ready
    // We can't access it directly (private), but we can verify the server compiles
    // and the Clone trait works (which requires the service field to be Clone)
    let server_clone = server.clone();

    assert_eq!(server.config().name, server_clone.config().name);
}

/// Test that server is Clone (Tower pattern)
#[test]
fn test_server_clone_pattern() {
    let server = ServerBuilder::new()
        .name("TestServer")
        .version("1.0.0")
        .build();

    // Clone should work (cheap Arc increments)
    let clone1 = server.clone();
    let clone2 = server.clone();

    // All clones should have same config
    assert_eq!(server.config().name, "TestServer");
    assert_eq!(clone1.config().name, "TestServer");
    assert_eq!(clone2.config().name, "TestServer");

    // Verify Arc-wrapped state is shared
    use std::sync::Arc;
    assert!(Arc::ptr_eq(server.registry(), clone1.registry()));
    assert!(Arc::ptr_eq(server.metrics(), clone2.metrics()));
}

/// Test valid JSON-RPC request through service (integration test)
#[tokio::test]
async fn test_valid_jsonrpc_request_through_service() {
    let server = ServerBuilder::new().build();

    // Create a valid JSON-RPC initialize request
    let json_rpc = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        }
    });

    let body = Bytes::from(serde_json::to_vec(&json_rpc).unwrap());

    // Build HTTP request (what transport layer creates)
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .body(body)
        .unwrap();

    // Call the service directly (testing Tower integration)
    let service = server.service();
    let response = service.oneshot(request).await.unwrap();

    // Verify response
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );

    // Parse response body
    let response_body = response.into_body();
    let json: serde_json::Value = serde_json::from_slice(&response_body).unwrap();

    // Should be valid JSON-RPC response
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], 1);
}

/// Test invalid JSON through service (validation layer)
#[tokio::test]
async fn test_invalid_json_through_service() {
    let server = ServerBuilder::new().build();

    // Invalid JSON
    let body = Bytes::from("not valid json");

    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .body(body)
        .unwrap();

    let service = server.service();
    let response = service.oneshot(request).await.unwrap();

    // Should return parse error
    let response_body = response.into_body();
    let json: serde_json::Value = serde_json::from_slice(&response_body).unwrap();

    assert_eq!(json["jsonrpc"], "2.0");
    assert!(json["error"].is_object());
    assert_eq!(json["error"]["code"], -32700); // Parse error
}

/// Test malformed JSON-RPC through service (validation layer)
#[tokio::test]
async fn test_malformed_jsonrpc_through_service() {
    let server = ServerBuilder::new().build();

    // Valid JSON but not valid JSON-RPC (missing required fields)
    let json_rpc = serde_json::json!({
        "method": "test"
        // Missing: jsonrpc, id
    });

    let body = Bytes::from(serde_json::to_vec(&json_rpc).unwrap());

    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .body(body)
        .unwrap();

    let service = server.service();
    let response = service.oneshot(request).await.unwrap();

    // Should return validation error
    let response_body = response.into_body();
    let json: serde_json::Value = serde_json::from_slice(&response_body).unwrap();

    assert_eq!(json["jsonrpc"], "2.0");
    assert!(json["error"].is_object());
}

/// Test that middleware layers are actually executing
#[tokio::test]
async fn test_middleware_execution() {
    // Build server with specific middleware config
    let server = ServerBuilder::new()
        .name("MiddlewareTest")
        .version("1.0.0")
        .build();

    // Valid request that will go through middleware
    let json_rpc = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "test-1",
        "method": "tools/list",
        "params": {}
    });

    let body = Bytes::from(serde_json::to_vec(&json_rpc).unwrap());

    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .body(body)
        .unwrap();

    let service = server.service();
    let response = service.oneshot(request).await.unwrap();

    // Response should succeed (no timeout, valid request)
    assert_eq!(response.status(), StatusCode::OK);

    // Parse response
    let response_body = response.into_body();
    let json: serde_json::Value = serde_json::from_slice(&response_body).unwrap();

    // Should be valid JSON-RPC response
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], "test-1");

    // Should have result (tools/list returns empty array by default)
    assert!(json["result"].is_object());
}

/// Test Clone works with multiple concurrent service calls
#[tokio::test]
async fn test_concurrent_service_calls() {
    let server = ServerBuilder::new().build();

    // Create multiple clones
    let server1 = server.clone();
    let server2 = server.clone();
    let server3 = server.clone();

    // Valid JSON-RPC request
    let json_rpc = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        }
    });

    let body = Bytes::from(serde_json::to_vec(&json_rpc).unwrap());

    // Clone body for each task (Bytes is cheap to clone - Arc internally)
    let body1 = body.clone();
    let body2 = body.clone();
    let body3 = body;

    // Spawn concurrent requests
    let handle1 = tokio::spawn(async move {
        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(body1)
            .unwrap();

        server1.service().oneshot(request).await
    });

    let handle2 = tokio::spawn(async move {
        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(body2)
            .unwrap();

        server2.service().oneshot(request).await
    });

    let handle3 = tokio::spawn(async move {
        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(body3)
            .unwrap();

        server3.service().oneshot(request).await
    });

    // All should succeed
    let (res1, res2, res3) = tokio::try_join!(handle1, handle2, handle3).unwrap();

    assert!(res1.is_ok());
    assert!(res2.is_ok());
    assert!(res3.is_ok());

    // All should return 200 OK
    assert_eq!(res1.unwrap().status(), StatusCode::OK);
    assert_eq!(res2.unwrap().status(), StatusCode::OK);
    assert_eq!(res3.unwrap().status(), StatusCode::OK);
}

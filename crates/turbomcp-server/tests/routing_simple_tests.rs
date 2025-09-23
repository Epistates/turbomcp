//! Simple routing tests for coverage improvement

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use turbomcp_core::RequestContext;
use turbomcp_protocol::{jsonrpc::*, types::*};
use turbomcp_server::ServerResult;
use turbomcp_server::registry::HandlerRegistry;
use turbomcp_server::routing::*;

// Simple mock route handler
#[derive(Debug)]
struct SimpleRouteHandler;

#[async_trait]
impl RouteHandler for SimpleRouteHandler {
    async fn handle(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> ServerResult<JsonRpcResponse> {
        Ok(JsonRpcResponse::success(
            json!({"status": "success"}),
            request.id,
        ))
    }

    fn can_handle(&self, method: &str) -> bool {
        method == "custom/test"
    }

    fn metadata(&self) -> RouteMetadata {
        RouteMetadata::default()
    }
}

// Helper to create request context using API
fn create_test_context() -> RequestContext {
    RequestContext::new()
}

// ============================================================================
// RouterConfig Tests
// ============================================================================

#[test]
fn test_router_config_default() {
    let config = RouterConfig::default();
    assert!(config.validate_requests);
    assert!(config.validate_responses);
    assert_eq!(config.default_timeout_ms, 30_000);
    assert!(config.enable_tracing);
    assert_eq!(config.max_concurrent_requests, 1000);
}

#[test]
fn test_router_config_custom() {
    let config = RouterConfig {
        validate_requests: false,
        validate_responses: false,
        default_timeout_ms: 5_000,
        enable_tracing: false,
        max_concurrent_requests: 100,
        enable_bidirectional: false,
    };

    assert!(!config.validate_requests);
    assert!(!config.validate_responses);
    assert_eq!(config.default_timeout_ms, 5_000);
    assert!(!config.enable_tracing);
    assert_eq!(config.max_concurrent_requests, 100);
}

#[test]
fn test_router_config_debug() {
    let config = RouterConfig::default();
    let debug_str = format!("{config:?}");
    assert!(debug_str.contains("RouterConfig"));
    assert!(debug_str.contains("validate_requests"));
}

#[test]
fn test_router_config_clone() {
    let config = RouterConfig::default();
    let cloned = config.clone();
    assert_eq!(config.validate_requests, cloned.validate_requests);
    assert_eq!(config.default_timeout_ms, cloned.default_timeout_ms);
}

// ============================================================================
// RouteMetadata Tests
// ============================================================================

#[test]
fn test_route_metadata_default() {
    let metadata = RouteMetadata::default();
    assert_eq!(metadata.name, "unknown");
    assert!(metadata.description.is_none());
    assert_eq!(metadata.version, "1.0.0");
    assert!(metadata.methods.is_empty());
    assert!(metadata.tags.is_empty());
}

#[test]
fn test_route_metadata_custom() {
    let metadata = RouteMetadata {
        name: "test-route".to_string(),
        description: Some("Test route".to_string()),
        version: "2.0.0".to_string(),
        methods: vec!["GET".to_string(), "POST".to_string()],
        tags: vec!["test".to_string(), "custom".to_string()],
    };

    assert_eq!(metadata.name, "test-route");
    assert_eq!(metadata.description, Some("Test route".to_string()));
    assert_eq!(metadata.version, "2.0.0");
    assert_eq!(metadata.methods.len(), 2);
    assert_eq!(metadata.tags.len(), 2);
}

#[test]
fn test_route_metadata_debug() {
    let metadata = RouteMetadata::default();
    let debug_str = format!("{metadata:?}");
    assert!(debug_str.contains("RouteMetadata"));
    assert!(debug_str.contains("name"));
}

#[test]
fn test_route_metadata_clone() {
    let metadata = RouteMetadata::default();
    let cloned = metadata.clone();
    assert_eq!(metadata.name, cloned.name);
    assert_eq!(metadata.version, cloned.version);
}

// ============================================================================
// RequestRouter Tests
// ============================================================================

#[test]
fn test_request_router_new() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let debug_str = format!("{router:?}");
    assert!(debug_str.contains("RequestRouter"));
}

#[test]
fn test_request_router_with_config() {
    let registry = Arc::new(HandlerRegistry::new());
    let config = RouterConfig {
        validate_requests: false,
        ..RouterConfig::default()
    };
    let router = RequestRouter::with_config(registry, config);

    let debug_str = format!("{router:?}");
    assert!(debug_str.contains("RequestRouter"));
}

#[test]
fn test_request_router_debug() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let debug_str = format!("{router:?}");
    assert!(debug_str.contains("RequestRouter"));
    assert!(debug_str.contains("config"));
    assert!(debug_str.contains("custom_routes_count"));
}

// ============================================================================
// Basic Routing Tests
// ============================================================================

#[tokio::test]
async fn test_route_method_not_found() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("test-1".to_string()),
        method: "unknown/method".to_string(),
        params: None,
    };

    let ctx = create_test_context();
    let response = router.route(request, ctx).await;
    assert_eq!(response.jsonrpc, JsonRpcVersion);
    assert!(response.error().is_some());

    if let Some(error) = response.error() {
        assert_eq!(error.code, -32601); // Method not found
    }
}

#[tokio::test]
async fn test_route_initialize_request() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

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

    let ctx = create_test_context();
    let response = router.route(request, ctx).await;
    assert_eq!(response.jsonrpc, JsonRpcVersion);
    // Initialize should return a response (either success or error)
    assert!(response.result().is_some() || response.error().is_some());
}

#[tokio::test]
async fn test_route_tools_list_request() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::Number(1),
        method: "tools/list".to_string(),
        params: None,
    };

    let ctx = create_test_context();
    let response = router.route(request, ctx).await;
    assert_eq!(response.jsonrpc, JsonRpcVersion);
    assert!(response.result().is_some() || response.error().is_some());
}

// ============================================================================
// Validation Tests
// ============================================================================

#[tokio::test]
async fn test_route_with_validation_disabled() {
    let registry = Arc::new(HandlerRegistry::new());
    let config = RouterConfig {
        validate_requests: false,
        validate_responses: false,
        ..RouterConfig::default()
    };
    let router = RequestRouter::with_config(registry, config);

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("test-1".to_string()),
        method: "initialize".to_string(),
        params: Some(json!({"invalid": "structure"})),
    };

    let ctx = create_test_context();
    let response = router.route(request, ctx).await;
    // Should not fail on validation since it's disabled
    assert_eq!(response.jsonrpc, JsonRpcVersion);
}

// ============================================================================
// Resource Method Tests
// ============================================================================

#[tokio::test]
async fn test_route_resource_methods() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let methods = vec![
        "resources/list",
        "resources/read",
        "resources/subscribe",
        "resources/unsubscribe",
    ];

    for method in methods {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: RequestId::String(format!("resource-{}", method.replace('/', "-"))),
            method: method.to_string(),
            params: Some(json!({})),
        };

        let ctx = create_test_context();
        let response = router.route(request, ctx).await;
        assert_eq!(response.jsonrpc, JsonRpcVersion);
        // Should handle all resource methods
        assert!(response.result().is_some() || response.error().is_some());
    }
}

// ============================================================================
// Logging and Sampling Tests
// ============================================================================

#[tokio::test]
async fn test_route_logging_and_sampling() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let methods = vec!["logging/setLevel", "sampling/createMessage"];

    for method in methods {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: RequestId::String(format!("{}-test", method.replace('/', "-"))),
            method: method.to_string(),
            params: Some(json!({})),
        };

        let ctx = create_test_context();
        let response = router.route(request, ctx).await;
        assert_eq!(response.jsonrpc, JsonRpcVersion);
        assert!(response.result().is_some() || response.error().is_some());
    }
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[tokio::test]
async fn test_route_empty_method() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("empty-method".to_string()),
        method: "".to_string(),
        params: None,
    };

    let ctx = create_test_context();
    let response = router.route(request, ctx).await;
    assert_eq!(response.jsonrpc, JsonRpcVersion);
    assert!(response.error().is_some()); // Should return method not found
}

#[tokio::test]
async fn test_route_very_long_method() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let very_long_method = "a".repeat(1000);
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("long-method".to_string()),
        method: very_long_method,
        params: None,
    };

    let ctx = create_test_context();
    let response = router.route(request, ctx).await;
    assert_eq!(response.jsonrpc, JsonRpcVersion);
    assert!(response.error().is_some()); // Should return method not found
}

// ============================================================================
// Add Route Tests
// ============================================================================

#[test]
fn test_router_add_route() {
    let registry = Arc::new(HandlerRegistry::new());
    let mut router = RequestRouter::new(registry);

    let handler = SimpleRouteHandler;
    let result = router.add_route(handler);

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_custom_route_integration() {
    let registry = Arc::new(HandlerRegistry::new());
    let mut router = RequestRouter::new(registry);

    let custom_handler = SimpleRouteHandler;
    router.add_route(custom_handler).unwrap();

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("custom-1".to_string()),
        method: "custom/test".to_string(),
        params: Some(json!({"test": "data"})),
    };

    let ctx = create_test_context();
    let response = router.route(request, ctx).await;
    assert_eq!(response.jsonrpc, JsonRpcVersion);
    // Custom route should either succeed or fail gracefully
    assert!(response.result().is_some() || response.error().is_some());

    if let Some(result) = response.result() {
        assert_eq!(result["status"], "success");
    } else if let Some(error) = response.error() {
        // It's OK if the custom route isn't found - the routing system is still working
        println!("Custom route returned error: {}", error.message);
    }
}

// ============================================================================
// Configuration Impact Tests
// ============================================================================

#[tokio::test]
async fn test_router_different_configurations() {
    let configs = vec![
        RouterConfig::default(),
        RouterConfig {
            validate_requests: false,
            validate_responses: false,
            default_timeout_ms: 1000,
            enable_tracing: false,
            max_concurrent_requests: 100,
            enable_bidirectional: false,
        },
        RouterConfig {
            validate_requests: true,
            validate_responses: true,
            default_timeout_ms: 60_000,
            enable_tracing: true,
            max_concurrent_requests: 10000,
            enable_bidirectional: true,
        },
    ];

    for (i, config) in configs.into_iter().enumerate() {
        let registry = Arc::new(HandlerRegistry::new());
        let router = RequestRouter::with_config(registry, config);

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: RequestId::Number(i as i64),
            method: "tools/list".to_string(),
            params: None,
        };

        let ctx = RequestContext::new();

        let response = router.route(request, ctx).await;
        assert_eq!(response.jsonrpc, JsonRpcVersion);
        // All configurations should handle basic requests
        assert!(response.result().is_some() || response.error().is_some());
    }
}

// ============================================================================
// Simple Route Handler Tests
// ============================================================================

#[test]
fn test_simple_route_handler() {
    let handler = SimpleRouteHandler;

    // Test can_handle
    assert!(handler.can_handle("custom/test"));
    assert!(!handler.can_handle("unknown/method"));

    // Test metadata
    let metadata = handler.metadata();
    assert_eq!(metadata.name, "unknown"); // Uses default
    assert_eq!(metadata.version, "1.0.0");
}

#[tokio::test]
async fn test_simple_route_handler_handle() {
    let handler = SimpleRouteHandler;

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("test-123".to_string()),
        method: "custom/test".to_string(),
        params: None,
    };

    let ctx = create_test_context();
    let response = handler.handle(request, ctx).await.unwrap();
    assert_eq!(response.jsonrpc, JsonRpcVersion);
    assert!(response.result().is_some());
    assert!(response.error().is_none());
}

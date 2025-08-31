//! Comprehensive tests for routing.rs to achieve 90%+ coverage
//! Targeting all routing scenarios, custom handlers, validation, and edge cases

use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use turbomcp_core::RequestContext;
use turbomcp_protocol::{jsonrpc::*, types::RequestId};
use turbomcp_server::{
    ServerError, ServerResult,
    registry::HandlerRegistry,
    routing::{RequestRouter, RouteHandler, RouteMetadata, RouterConfig},
};

// ========== Helper Setup ==========

fn create_test_context() -> RequestContext {
    RequestContext::new()
        .with_user_id("test-user".to_string())
        .with_session_id("test-session".to_string())
        .with_client_id("test-client".to_string())
}

#[allow(dead_code)]
fn create_test_context_with_roles(roles: Vec<&str>) -> RequestContext {
    let ctx = create_test_context();
    let mut auth_metadata = HashMap::new();
    let role_values: Vec<Value> = roles.iter().map(|r| Value::String(r.to_string())).collect();
    auth_metadata.insert("roles".to_string(), Value::Array(role_values));

    let auth_value = Value::Object(auth_metadata.into_iter().collect());

    // Create new context with auth metadata
    ctx.with_metadata("auth".to_string(), auth_value)
}

fn create_basic_request(method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        method: method.to_string(),
        params,
        id: RequestId::String("test-1".to_string()),
    }
}

// ========== Mock Custom Route Handler ==========

#[derive(Debug)]
struct MockCustomHandler {
    methods: Vec<String>,
    should_succeed: bool,
    response_data: Value,
}

impl MockCustomHandler {
    fn new(methods: Vec<String>, should_succeed: bool, response_data: Value) -> Self {
        Self {
            methods,
            should_succeed,
            response_data,
        }
    }
}

#[async_trait::async_trait]
impl RouteHandler for MockCustomHandler {
    async fn handle(
        &self,
        _request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> ServerResult<JsonRpcResponse> {
        if self.should_succeed {
            Ok(JsonRpcResponse {
                jsonrpc: JsonRpcVersion,
                id: Some(RequestId::String("test".to_string())),
                result: Some(self.response_data.clone()),
                error: None,
            })
        } else {
            Err(ServerError::Internal("Mock handler error".to_string()))
        }
    }

    fn can_handle(&self, method: &str) -> bool {
        self.methods.contains(&method.to_string())
    }

    fn metadata(&self) -> RouteMetadata {
        RouteMetadata {
            name: "mock-handler".to_string(),
            description: Some("Mock custom handler".to_string()),
            version: "1.0.0".to_string(),
            methods: self.methods.clone(),
            tags: vec!["mock".to_string(), "test".to_string()],
        }
    }
}

// ========== Router Configuration Tests ==========

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
        default_timeout_ms: 60_000,
        enable_tracing: false,
        max_concurrent_requests: 500,
        enable_bidirectional: false,
    };

    assert!(!config.validate_requests);
    assert!(!config.validate_responses);
    assert_eq!(config.default_timeout_ms, 60_000);
    assert!(!config.enable_tracing);
    assert_eq!(config.max_concurrent_requests, 500);
}

#[test]
fn test_router_config_clone_debug() {
    let config = RouterConfig::default();
    let cloned = config.clone();
    assert_eq!(config.default_timeout_ms, cloned.default_timeout_ms);

    let debug_str = format!("{config:?}");
    assert!(debug_str.contains("RouterConfig"));
    assert!(debug_str.contains("validate_requests"));
}

// ========== Route Metadata Tests ==========

#[test]
fn test_route_metadata_default() {
    let metadata = RouteMetadata::default();
    assert_eq!(metadata.name, "unknown");
    assert_eq!(metadata.description, None);
    assert_eq!(metadata.version, "1.0.0");
    assert!(metadata.methods.is_empty());
    assert!(metadata.tags.is_empty());
}

#[test]
fn test_route_metadata_custom() {
    let metadata = RouteMetadata {
        name: "custom-handler".to_string(),
        description: Some("Custom route handler".to_string()),
        version: "2.1.0".to_string(),
        methods: vec!["custom/method1".to_string(), "custom/method2".to_string()],
        tags: vec!["custom".to_string(), "advanced".to_string()],
    };

    assert_eq!(metadata.name, "custom-handler");
    assert_eq!(
        metadata.description,
        Some("Custom route handler".to_string())
    );
    assert_eq!(metadata.version, "2.1.0");
    assert_eq!(metadata.methods.len(), 2);
    assert_eq!(metadata.tags.len(), 2);
}

#[test]
fn test_route_metadata_clone_debug() {
    let metadata = RouteMetadata::default();
    let cloned = metadata.clone();
    assert_eq!(metadata.name, cloned.name);

    let debug_str = format!("{metadata:?}");
    assert!(debug_str.contains("RouteMetadata"));
}

// ========== Basic Router Tests ==========

#[test]
fn test_router_creation() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry.clone());

    let debug_str = format!("{router:?}");
    assert!(debug_str.contains("RequestRouter"));
}

#[test]
fn test_router_with_config() {
    let registry = Arc::new(HandlerRegistry::new());
    let config = RouterConfig {
        validate_requests: false,
        validate_responses: true,
        default_timeout_ms: 45_000,
        enable_tracing: false,
        max_concurrent_requests: 750,
        enable_bidirectional: false,
    };

    let router = RequestRouter::with_config(registry, config);
    let debug_str = format!("{router:?}");
    assert!(debug_str.contains("RequestRouter"));
}

#[test]
fn test_router_clone() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);
    let cloned = router.clone();

    // Verify both routers work independently
    let debug1 = format!("{router:?}");
    let debug2 = format!("{cloned:?}");
    assert!(debug1.contains("RequestRouter"));
    assert!(debug2.contains("RequestRouter"));
}

// ========== Custom Route Handler Tests ==========

#[tokio::test]
async fn test_add_custom_route_success() {
    let registry = Arc::new(HandlerRegistry::new());
    let mut router = RequestRouter::new(registry);

    let handler = MockCustomHandler::new(
        vec!["custom/test".to_string()],
        true,
        json!({"status": "success"}),
    );

    let result = router.add_route(handler);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_add_custom_route_duplicate_method() {
    let registry = Arc::new(HandlerRegistry::new());
    let mut router = RequestRouter::new(registry);

    let handler1 = MockCustomHandler::new(
        vec!["custom/duplicate".to_string()],
        true,
        json!({"handler": "first"}),
    );

    let handler2 = MockCustomHandler::new(
        vec!["custom/duplicate".to_string()],
        true,
        json!({"handler": "second"}),
    );

    assert!(router.add_route(handler1).is_ok());
    let result = router.add_route(handler2);
    assert!(result.is_err());

    if let Err(e) = result {
        let error_str = e.to_string();
        assert!(error_str.contains("already exists"));
    }
}

#[tokio::test]
async fn test_custom_route_handler_execution() {
    let registry = Arc::new(HandlerRegistry::new());
    let mut router = RequestRouter::new(registry);

    let handler = MockCustomHandler::new(
        vec!["custom/success".to_string()],
        true,
        json!({"message": "custom handler worked"}),
    );

    router.add_route(handler).unwrap();

    let request = create_basic_request("custom/success", None);
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_some());
    assert!(response.error.is_none());

    if let Some(result) = response.result {
        assert_eq!(result["message"], "custom handler worked");
    }
}

#[tokio::test]
async fn test_custom_route_handler_error() {
    let registry = Arc::new(HandlerRegistry::new());
    let mut router = RequestRouter::new(registry);

    let handler = MockCustomHandler::new(vec!["custom/error".to_string()], false, json!({}));

    router.add_route(handler).unwrap();

    let request = create_basic_request("custom/error", None);
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_none());
    assert!(response.error.is_some());
}

// ========== Protocol Method Tests ==========

#[tokio::test]
async fn test_handle_initialize() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let init_params = json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "sampling": null
        },
        "clientInfo": {
            "name": "test-client",
            "version": "1.0.0"
        }
    });

    let request = create_basic_request("initialize", Some(init_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;

    // Debug the response if it fails
    if response.error.is_some() {
        println!("Initialize error: {:?}", response.error);
    }

    assert!(response.result.is_some());
    assert!(response.error.is_none());

    if let Some(result) = response.result {
        assert!(result.get("protocolVersion").is_some());
        assert!(result.get("serverInfo").is_some());
        assert!(result.get("capabilities").is_some());
    }
}

#[tokio::test]
async fn test_handle_initialize_missing_params() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = create_basic_request("initialize", None);
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    if let Some(error) = response.error {
        assert!(error.message.contains("Missing required parameters"));
    }
}

#[tokio::test]
async fn test_handle_list_tools_empty() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = create_basic_request("tools/list", Some(json!({})));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_some());
    assert!(response.error.is_none());

    if let Some(result) = response.result {
        assert!(result.get("tools").is_some());
        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
            assert!(tools.is_empty());
        }
    }
}

#[tokio::test]
async fn test_handle_call_tool_not_found() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let call_params = json!({
        "name": "nonexistent_tool",
        "arguments": {}
    });

    let request = create_basic_request("tools/call", Some(call_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    if let Some(error) = response.error {
        assert!(error.message.contains("Tool 'nonexistent_tool'"));
    }
}

#[tokio::test]
async fn test_handle_list_prompts_empty() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = create_basic_request("prompts/list", Some(json!({})));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_some());
    assert!(response.error.is_none());
}

#[tokio::test]
async fn test_handle_get_prompt_not_found() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let prompt_params = json!({
        "name": "nonexistent_prompt",
        "arguments": {}
    });

    let request = create_basic_request("prompts/get", Some(prompt_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    if let Some(error) = response.error {
        assert!(error.message.contains("Prompt 'nonexistent_prompt'"));
    }
}

#[tokio::test]
async fn test_handle_list_resources_empty() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = create_basic_request("resources/list", Some(json!({})));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_some());
    assert!(response.error.is_none());
}

#[tokio::test]
async fn test_handle_read_resource_not_found() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let resource_params = json!({
        "uri": "file:///nonexistent/resource.txt"
    });

    let request = create_basic_request("resources/read", Some(resource_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_none());
    assert!(response.error.is_some());
}

// ========== Resource Subscription Tests ==========

#[tokio::test]
async fn test_resource_subscribe_success() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let sub_params = json!({
        "uri": "file:///test/resource.txt"
    });

    let request = create_basic_request("resources/subscribe", Some(sub_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_some());
    assert!(response.error.is_none());
}

#[tokio::test]
async fn test_resource_subscribe_multiple() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let uri = "file:///test/multi_resource.txt";

    // Subscribe multiple times to the same resource
    for i in 0..3 {
        let sub_params = json!({
            "uri": uri
        });

        let request = create_basic_request("resources/subscribe", Some(sub_params));
        let ctx = create_test_context();

        let response = router.route(request, ctx).await;
        assert!(response.result.is_some(), "Subscription {} failed", i + 1);
    }
}

#[tokio::test]
async fn test_resource_unsubscribe_success() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let uri = "file:///test/unsub_resource.txt";

    // First subscribe
    let sub_params = json!({
        "uri": uri
    });
    let sub_request = create_basic_request("resources/subscribe", Some(sub_params));
    let ctx = create_test_context();
    let sub_response = router.route(sub_request, ctx.clone()).await;
    assert!(sub_response.result.is_some());

    // Then unsubscribe
    let unsub_params = json!({
        "uri": uri
    });
    let unsub_request = create_basic_request("resources/unsubscribe", Some(unsub_params));
    let unsub_response = router.route(unsub_request, ctx).await;
    assert!(unsub_response.result.is_some());
    assert!(unsub_response.error.is_none());
}

#[tokio::test]
async fn test_resource_unsubscribe_nonexistent() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let unsub_params = json!({
        "uri": "file:///test/never_subscribed.txt"
    });

    let request = create_basic_request("resources/unsubscribe", Some(unsub_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_some()); // Should still succeed
    assert!(response.error.is_none());
}

#[tokio::test]
async fn test_resource_subscription_counter_management() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let uri = "file:///test/counter_resource.txt";

    // Subscribe twice
    for _ in 0..2 {
        let sub_params = json!({"uri": uri});
        let request = create_basic_request("resources/subscribe", Some(sub_params));
        let ctx = create_test_context();
        router.route(request, ctx).await;
    }

    // Unsubscribe once (should still be subscribed)
    let unsub_params = json!({"uri": uri});
    let unsub_request = create_basic_request("resources/unsubscribe", Some(unsub_params));
    let ctx = create_test_context();
    let response = router.route(unsub_request, ctx.clone()).await;
    assert!(response.result.is_some());

    // Unsubscribe again (should be fully unsubscribed)
    let unsub_params2 = json!({"uri": uri});
    let unsub_request2 = create_basic_request("resources/unsubscribe", Some(unsub_params2));
    let response2 = router.route(unsub_request2, ctx).await;
    assert!(response2.result.is_some());
}

// ========== Logging and Sampling Handler Tests ==========

#[tokio::test]
async fn test_handle_set_log_level_no_handler() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let level_params = json!({
        "level": "info"
    });

    let request = create_basic_request("logging/setLevel", Some(level_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    if let Some(error) = response.error {
        assert!(error.message.contains("No logging handler available"));
    }
}

#[tokio::test]
async fn test_handle_create_message_no_handler() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let message_params = json!({
        "maxTokens": 100,
        "messages": [
            {
                "role": "user",
                "content": {
                    "type": "text",
                    "text": "Hello, world!"
                }
            }
        ]
    });

    let request = create_basic_request("sampling/createMessage", Some(message_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    if let Some(error) = response.error {
        assert!(error.message.contains("No sampling handler available"));
    }
}

// ========== List Roots Tests ==========

#[tokio::test]
async fn test_handle_list_roots() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = create_basic_request("roots/list", Some(json!({})));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_some());
    assert!(response.error.is_none());

    if let Some(result) = response.result {
        assert!(result.get("roots").is_some());
        if let Some(roots) = result.get("roots").and_then(|r| r.as_array()) {
            // Should have at least one root on any OS
            assert!(!roots.is_empty());
        }
    }
}

// ========== Method Not Found Tests ==========

#[tokio::test]
async fn test_method_not_found() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = create_basic_request("nonexistent/method", Some(json!({})));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    if let Some(error) = response.error {
        assert_eq!(error.code, -32601); // Method not found
        assert!(
            error
                .message
                .contains("Method 'nonexistent/method' not found")
        );
    }
}

// ========== Validation Tests ==========

#[tokio::test]
async fn test_request_validation_disabled() {
    let registry = Arc::new(HandlerRegistry::new());
    let config = RouterConfig {
        validate_requests: false,
        validate_responses: true,
        ..RouterConfig::default()
    };
    let router = RequestRouter::with_config(registry, config);

    // Send malformed request
    let mut malformed_request = create_basic_request("initialize", None);
    malformed_request.method = "".to_string(); // Invalid empty method

    let ctx = create_test_context();
    let response = router.route(malformed_request, ctx).await;

    // Should get an error since the method is empty and validation is disabled
    // The error might be "Method not found" or similar since validation is disabled
    assert!(response.error.is_some());
}

#[tokio::test]
async fn test_response_validation_disabled() {
    let registry = Arc::new(HandlerRegistry::new());
    let config = RouterConfig {
        validate_requests: true,
        validate_responses: false,
        ..RouterConfig::default()
    };
    let router = RequestRouter::with_config(registry, config);

    let request = create_basic_request("tools/list", Some(json!({})));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_some());
}

// ========== Batch Request Tests ==========

#[tokio::test]
async fn test_route_batch_empty() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let ctx = create_test_context();
    let responses = router.route_batch(vec![], ctx).await;
    assert!(responses.is_empty());
}

#[tokio::test]
async fn test_route_batch_single_request() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = create_basic_request("tools/list", Some(json!({})));
    let ctx = create_test_context();

    let responses = router.route_batch(vec![request], ctx).await;
    assert_eq!(responses.len(), 1);
    assert!(responses[0].result.is_some());
}

#[tokio::test]
async fn test_route_batch_multiple_requests() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let requests = vec![
        create_basic_request("tools/list", Some(json!({}))),
        create_basic_request("prompts/list", Some(json!({}))),
        create_basic_request("resources/list", Some(json!({}))),
    ];

    let ctx = create_test_context();
    let responses = router.route_batch(requests, ctx).await;
    assert_eq!(responses.len(), 3);

    for response in responses {
        assert!(response.result.is_some());
    }
}

#[tokio::test]
async fn test_route_batch_with_errors() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let requests = vec![
        create_basic_request("tools/list", Some(json!({}))), // Should succeed
        create_basic_request("nonexistent/method", Some(json!({}))), // Should fail
        create_basic_request("prompts/list", Some(json!({}))), // Should succeed
    ];

    let ctx = create_test_context();
    let responses = router.route_batch(requests, ctx).await;
    assert_eq!(responses.len(), 3);

    assert!(responses[0].result.is_some()); // tools/list
    assert!(responses[1].error.is_some()); // nonexistent/method
    assert!(responses[2].result.is_some()); // prompts/list
}

#[tokio::test]
async fn test_route_batch_concurrent_limit() {
    let registry = Arc::new(HandlerRegistry::new());
    let config = RouterConfig {
        max_concurrent_requests: 2, // Low limit for testing
        ..RouterConfig::default()
    };
    let router = RequestRouter::with_config(registry, config);

    // Create more requests than the concurrent limit
    let requests = vec![
        create_basic_request("tools/list", Some(json!({}))),
        create_basic_request("prompts/list", Some(json!({}))),
        create_basic_request("resources/list", Some(json!({}))),
        create_basic_request("tools/list", Some(json!({}))),
        create_basic_request("prompts/list", Some(json!({}))),
    ];

    let ctx = create_test_context();
    let responses = router.route_batch(requests, ctx).await;
    assert_eq!(responses.len(), 5);

    // All should succeed despite the limit
    for response in responses {
        assert!(response.result.is_some());
    }
}

// ========== Parameter Parsing Tests ==========

#[tokio::test]
async fn test_parse_params_invalid_json() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let init_params = json!({
        "protocolVersion": 123, // Should be string
        "capabilities": "invalid", // Should be object
    });

    let request = create_basic_request("initialize", Some(init_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    if let Some(error) = response.error {
        assert!(error.message.contains("Invalid parameters"));
    }
}

// ========== URI Pattern Matching Tests (via resource reading) ==========

#[tokio::test]
async fn test_resource_pattern_matching_via_read() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    // Test that exact URI patterns work through resource reading
    let resource_params = json!({
        "uri": "file:///exact/pattern/test.txt"
    });

    let request = create_basic_request("resources/read", Some(resource_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    // Should get "not found" since no handlers registered
    assert!(response.error.is_some());
    if let Some(error) = response.error {
        assert!(error.message.contains("Resource"));
    }
}

// ========== Server Capabilities Tests (via initialize) ==========

#[tokio::test]
async fn test_server_capabilities_via_initialize() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let init_params = json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {
            "name": "test-client",
            "version": "1.0.0"
        }
    });

    let request = create_basic_request("initialize", Some(init_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_some());

    if let Some(result) = response.result {
        let capabilities = result.get("capabilities");
        assert!(capabilities.is_some());

        // With empty registry, most capabilities should be None/absent
        if let Some(caps) = capabilities {
            // Tools capability should be absent since no tools registered
            assert!(caps.get("tools").is_none() || caps.get("tools").unwrap().is_null());
        }
    }
}

// ========== Error Response Tests (via public API) ==========

#[tokio::test]
async fn test_error_response_via_invalid_method() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = create_basic_request("unknown/method", Some(json!({})));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    if let Some(error) = response.error {
        assert_eq!(error.code, -32601);
        assert!(error.message.contains("Method 'unknown/method' not found"));
    }
}

#[tokio::test]
async fn test_success_response_via_valid_request() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let request = create_basic_request("tools/list", Some(json!({})));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.result.is_some());
    assert!(response.error.is_none());
    assert_eq!(response.id, Some(RequestId::String("test-1".to_string())));
}

// ========== Edge Cases and Error Paths ==========

#[tokio::test]
async fn test_invalid_subscribe_params() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let invalid_params = json!({
        "not_uri": "file:///test.txt"
    });

    let request = create_basic_request("resources/subscribe", Some(invalid_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.error.is_some());
}

#[tokio::test]
async fn test_invalid_unsubscribe_params() {
    let registry = Arc::new(HandlerRegistry::new());
    let router = RequestRouter::new(registry);

    let invalid_params = json!({
        "not_uri": "file:///test.txt"
    });

    let request = create_basic_request("resources/unsubscribe", Some(invalid_params));
    let ctx = create_test_context();

    let response = router.route(request, ctx).await;
    assert!(response.error.is_some());
}

#[tokio::test]
async fn test_custom_handler_multiple_methods() {
    let registry = Arc::new(HandlerRegistry::new());
    let mut router = RequestRouter::new(registry);

    let handler = MockCustomHandler::new(
        vec!["custom/method1".to_string(), "custom/method2".to_string()],
        true,
        json!({"multi": "method"}),
    );

    router.add_route(handler).unwrap();

    // Test both methods work
    for method in &["custom/method1", "custom/method2"] {
        let request = create_basic_request(method, None);
        let ctx = create_test_context();

        let response = router.route(request, ctx).await;
        assert!(response.result.is_some());

        if let Some(result) = response.result {
            assert_eq!(result["multi"], "method");
        }
    }
}

#[test]
fn test_route_debug_formatting() {
    use turbomcp_server::routing::{Route, RouteMetadata};

    let handler = MockCustomHandler::new(vec!["test/method".to_string()], true, json!({}));

    let route = Route {
        method: "test/method".to_string(),
        handler: Arc::new(handler),
        metadata: RouteMetadata::default(),
    };

    let debug_str = format!("{route:?}");
    assert!(debug_str.contains("Route"));
    assert!(debug_str.contains("test/method"));
}

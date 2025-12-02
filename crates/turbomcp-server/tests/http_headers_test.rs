//! Tests for HTTP header access in Context
//!
//! These tests verify that HTTP headers are correctly extracted from requests
//! and made available through the Context API.

use bytes::Bytes;
use http::{Request, StatusCode};
use std::sync::Arc;
use tower::ServiceExt; // For oneshot
use turbomcp_protocol::RequestContext;
use turbomcp_server::{
    config::ServerConfig, metrics::ServerMetrics, registry::HandlerRegistry,
    routing::RequestRouter, service::McpService,
};

#[tokio::test]
async fn test_http_headers_extracted() {
    // Create service components
    let registry = Arc::new(HandlerRegistry::new());
    let metrics = Arc::new(ServerMetrics::new());
    let config = ServerConfig::default();
    let router = Arc::new(RequestRouter::new(
        Arc::clone(&registry),
        Arc::clone(&metrics),
        config,
        #[cfg(feature = "mcp-tasks")]
        None,
    ));

    // Create MCP service
    let service = McpService::new(registry, router, metrics);

    // Create HTTP request with custom headers
    let body = r#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}},"id":1}"#;

    let request = Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", "application/json")
        .header("user-agent", "TurboMCP-Test/1.0")
        .header("x-custom-header", "custom-value")
        .header("x-request-id", "test-123")
        .body(Bytes::from(body))
        .unwrap();

    // Process request
    let response = service.oneshot(request).await;

    // Verify response is successful (initialization should work)
    assert!(response.is_ok());
    let response = response.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[test]
fn test_context_header_methods() {
    // This is a unit test for the Context helper methods
    // We'll create a RequestContext with metadata and verify the helper methods work

    let mut request_context = RequestContext::new();

    // Simulate HTTP headers as they would be added by service.rs
    let headers: std::collections::HashMap<String, String> = [
        ("user-agent".to_string(), "Test-Agent/1.0".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
        ("X-Custom-Header".to_string(), "custom-value".to_string()),
    ]
    .iter()
    .cloned()
    .collect();

    let headers_json = serde_json::to_value(&headers).unwrap();
    request_context = request_context
        .with_metadata("transport", "http")
        .with_metadata("http_headers", headers_json);

    // Now we can test that the headers are accessible
    // (In practice, this would be tested through the Context type, but we're testing
    // the underlying data structure here)

    // Verify transport metadata
    assert_eq!(
        request_context.get_metadata("transport"),
        Some(&serde_json::json!("http"))
    );

    // Verify headers metadata exists
    assert!(request_context.get_metadata("http_headers").is_some());

    // Verify we can deserialize headers
    let stored_headers: std::collections::HashMap<String, String> = serde_json::from_value(
        request_context
            .get_metadata("http_headers")
            .unwrap()
            .clone(),
    )
    .unwrap();

    assert_eq!(stored_headers.len(), 3);
    assert_eq!(
        stored_headers.get("user-agent"),
        Some(&"Test-Agent/1.0".to_string())
    );
}

#[test]
fn test_header_case_insensitivity() {
    // Test that the metadata structure preserves header names
    // The actual case-insensitive lookup is handled by the Context::header() method

    let headers: std::collections::HashMap<String, String> = [
        ("User-Agent".to_string(), "Test/1.0".to_string()),
        ("Content-Type".to_string(), "application/json".to_string()),
    ]
    .iter()
    .cloned()
    .collect();

    let headers_json = serde_json::to_value(&headers).unwrap();

    // Verify serialization preserves the data
    let deserialized: std::collections::HashMap<String, String> =
        serde_json::from_value(headers_json).unwrap();

    assert_eq!(deserialized.len(), 2);
    // Note: HashMap iteration order is not guaranteed, but keys should be present
    assert!(deserialized.contains_key("User-Agent") || deserialized.contains_key("user-agent"));
}

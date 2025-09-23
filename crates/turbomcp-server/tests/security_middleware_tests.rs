//! Tests for security headers middleware

use serde_json::json;
use std::collections::HashMap;
use turbomcp_core::{MessageId, RequestContext};
use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcVersion};
use turbomcp_server::ServerResult;
use turbomcp_server::middleware::{Middleware, SecurityHeadersConfig, SecurityHeadersMiddleware};

#[tokio::test]
async fn test_security_headers_default_config() -> ServerResult<()> {
    let middleware = SecurityHeadersMiddleware::new();
    let mut request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        method: "test".to_string(),
        params: None,
        id: MessageId::from("test-1"),
    };
    let mut ctx = RequestContext::new();

    // Request processing should do nothing
    middleware.process_request(&mut request, &mut ctx).await?;

    // Response processing should add security headers
    let mut response = JsonRpcResponse::success(json!({"status": "ok"}), MessageId::from("test-1"));

    middleware.process_response(&mut response, &ctx).await?;

    // Check that security headers were added to response
    let result = response.result().unwrap();
    let security_headers = result.get("_security_headers").unwrap();

    assert!(security_headers.get("Content-Security-Policy").is_some());
    assert!(security_headers.get("X-Frame-Options").is_some());
    assert!(security_headers.get("X-Content-Type-Options").is_some());
    assert!(security_headers.get("X-XSS-Protection").is_some());
    assert!(security_headers.get("Strict-Transport-Security").is_some());
    assert!(security_headers.get("Referrer-Policy").is_some());
    assert!(security_headers.get("Permissions-Policy").is_some());

    // Verify specific header values
    let csp = security_headers
        .get("Content-Security-Policy")
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(csp.contains("default-src 'self'"));

    let xfo = security_headers
        .get("X-Frame-Options")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(xfo, "DENY");

    let nosniff = security_headers
        .get("X-Content-Type-Options")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(nosniff, "nosniff");

    Ok(())
}

#[tokio::test]
async fn test_security_headers_relaxed_config() -> ServerResult<()> {
    let middleware = SecurityHeadersMiddleware::relaxed();
    let mut request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        method: "test".to_string(),
        params: None,
        id: MessageId::from("test-2"),
    };
    let mut ctx = RequestContext::new();

    middleware.process_request(&mut request, &mut ctx).await?;

    let mut response = JsonRpcResponse::success(json!({"status": "ok"}), MessageId::from("test-2"));

    middleware.process_response(&mut response, &ctx).await?;

    // Check relaxed configuration
    let result = response.result().unwrap();
    let security_headers = result.get("_security_headers").unwrap();

    let csp = security_headers
        .get("Content-Security-Policy")
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(csp.contains("'unsafe-inline'"));
    assert!(csp.contains("'unsafe-eval'"));

    let xfo = security_headers
        .get("X-Frame-Options")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(xfo, "SAMEORIGIN");

    // HSTS should not be set in relaxed mode
    assert!(security_headers.get("Strict-Transport-Security").is_none());

    Ok(())
}

#[tokio::test]
async fn test_security_headers_strict_config() -> ServerResult<()> {
    let middleware = SecurityHeadersMiddleware::strict();
    let mut request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        method: "test".to_string(),
        params: None,
        id: MessageId::from("test-3"),
    };
    let mut ctx = RequestContext::new();

    middleware.process_request(&mut request, &mut ctx).await?;

    let mut response = JsonRpcResponse::success(json!({"status": "ok"}), MessageId::from("test-3"));

    middleware.process_response(&mut response, &ctx).await?;

    // Check strict configuration
    let result = response.result().unwrap();
    let security_headers = result.get("_security_headers").unwrap();

    let csp = security_headers
        .get("Content-Security-Policy")
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(csp.contains("default-src 'none'"));
    assert!(!csp.contains("'unsafe-inline'"));

    let referrer = security_headers
        .get("Referrer-Policy")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(referrer, "no-referrer");

    let hsts = security_headers
        .get("Strict-Transport-Security")
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(hsts.contains("max-age=63072000"));

    Ok(())
}

#[tokio::test]
async fn test_security_headers_custom_config() -> ServerResult<()> {
    let mut custom_headers = HashMap::new();
    custom_headers.insert("X-Custom-Header".to_string(), "CustomValue".to_string());
    custom_headers.insert("X-API-Version".to_string(), "v1.0".to_string());

    let config = SecurityHeadersConfig::new()
        .with_csp(Some("default-src 'self' *.example.com".to_string()))
        .with_hsts(Some("max-age=86400".to_string()))
        .with_custom_header("X-Custom-Header".to_string(), "CustomValue".to_string())
        .with_custom_header("X-API-Version".to_string(), "v1.0".to_string());

    let middleware = SecurityHeadersMiddleware::with_config(config);
    let mut request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        method: "test".to_string(),
        params: None,
        id: MessageId::from("test-4"),
    };
    let mut ctx = RequestContext::new();

    middleware.process_request(&mut request, &mut ctx).await?;

    let mut response = JsonRpcResponse::success(json!({"status": "ok"}), MessageId::from("test-4"));

    middleware.process_response(&mut response, &ctx).await?;

    // Check custom CSP
    let result = response.result().unwrap();
    let security_headers = result.get("_security_headers").unwrap();

    let csp = security_headers
        .get("Content-Security-Policy")
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(csp.contains("*.example.com"));

    // Check custom HSTS
    let hsts = security_headers
        .get("Strict-Transport-Security")
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(hsts.contains("max-age=86400"));

    // Check custom headers
    let custom_header = security_headers
        .get("X-Custom-Header")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(custom_header, "CustomValue");

    let api_version = security_headers
        .get("X-API-Version")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(api_version, "v1.0");

    Ok(())
}

#[tokio::test]
async fn test_security_headers_middleware_properties() {
    let middleware = SecurityHeadersMiddleware::new();

    assert_eq!(middleware.name(), "security_headers");
    assert_eq!(middleware.priority(), 900);
    assert!(middleware.enabled());
}

#[tokio::test]
async fn test_security_headers_config_builders() {
    // Test SecurityHeadersConfig::relaxed()
    let relaxed = SecurityHeadersConfig::relaxed();
    assert!(
        relaxed
            .content_security_policy
            .as_ref()
            .unwrap()
            .contains("'unsafe-inline'")
    );
    assert_eq!(relaxed.x_frame_options.as_ref().unwrap(), "SAMEORIGIN");
    assert!(relaxed.strict_transport_security.is_none());

    // Test SecurityHeadersConfig::strict()
    let strict = SecurityHeadersConfig::strict();
    assert!(
        strict
            .content_security_policy
            .as_ref()
            .unwrap()
            .contains("default-src 'none'")
    );
    assert_eq!(strict.x_frame_options.as_ref().unwrap(), "DENY");
    assert_eq!(strict.referrer_policy.as_ref().unwrap(), "no-referrer");

    // Test builder methods
    let custom = SecurityHeadersConfig::new().with_csp(None).with_hsts(None);
    assert!(custom.content_security_policy.is_none());
    assert!(custom.strict_transport_security.is_none());
}

#[tokio::test]
async fn test_security_headers_integration() -> ServerResult<()> {
    // Test that the middleware can be used in a middleware stack
    use turbomcp_server::middleware::MiddlewareStack;

    let mut stack = MiddlewareStack::new();
    stack.add(SecurityHeadersMiddleware::new());

    assert_eq!(stack.len(), 1);
    assert_eq!(stack.list_middleware(), vec!["security_headers"]);

    // Test request/response processing through the stack
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        method: "test".to_string(),
        params: None,
        id: MessageId::from("test-5"),
    };
    let ctx = RequestContext::new();

    let (_request, ctx) = stack.process_request(request, ctx).await?;

    let response = JsonRpcResponse::success(json!({"status": "ok"}), MessageId::from("test-5"));

    let response = stack.process_response(response, &ctx).await?;

    // Verify security headers were applied
    let result = response.result().unwrap();
    assert!(result.get("_security_headers").is_some());

    Ok(())
}

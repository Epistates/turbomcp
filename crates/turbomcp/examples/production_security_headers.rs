//! Security Headers Middleware Demo
//!
//! This example demonstrates how to use the SecurityHeadersMiddleware
//! to add HTTP security headers for defense-in-depth protection.

use serde_json::json;
use turbomcp_server::middleware::{SecurityHeadersConfig, SecurityHeadersMiddleware};
use turbomcp_server::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🔒 TurboMCP Security Headers Middleware Demo");
    println!("============================================");

    // Create different security configurations
    println!("\n📋 Security Configuration Options:");

    // 1. Default configuration (balanced security)
    let _default_middleware = SecurityHeadersMiddleware::new();
    println!("├─ Default: Balanced security for most applications");

    // 2. Relaxed configuration (development-friendly)
    let _relaxed_middleware = SecurityHeadersMiddleware::relaxed();
    println!("├─ Relaxed: Development-friendly with looser policies");

    // 3. Strict configuration (maximum security)
    let strict_middleware = SecurityHeadersMiddleware::strict();
    println!("├─ Strict: Maximum security for production environments");

    // 4. Custom configuration
    let custom_config = SecurityHeadersConfig::new()
        .with_csp(Some(
            "default-src 'self'; script-src 'self' 'nonce-abc123'; style-src 'self' 'unsafe-inline'".to_string()
        ))
        .with_hsts(Some("max-age=31536000; includeSubDomains".to_string()))
        .with_custom_header("X-API-Version".to_string(), "v2.0".to_string())
        .with_custom_header("X-Request-ID".to_string(), "req-123456".to_string());

    let _custom_middleware = SecurityHeadersMiddleware::with_config(custom_config);
    println!("└─ Custom: Tailored security headers for specific needs");

    // Demonstrate middleware stack integration
    println!("\n🔧 Middleware Stack Integration:");

    let mut stack = MiddlewareStack::new();

    // Add security headers middleware (high priority)
    stack.add(SecurityHeadersMiddleware::strict());

    // Add logging middleware (lower priority)
    stack.add(LoggingMiddleware::new());

    println!(
        "├─ Added SecurityHeadersMiddleware (priority: {})",
        strict_middleware.priority()
    );
    println!("├─ Added LoggingMiddleware");
    println!("├─ Stack size: {} middleware layers", stack.len());
    println!("└─ Middleware order: {:?}", stack.list_middleware());

    // Simulate processing a request through the middleware stack
    println!("\n🚀 Processing Sample Request:");

    use turbomcp_core::{MessageId, RequestContext};
    use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcVersion};

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        method: "tools/list".to_string(),
        params: None,
        id: MessageId::from("demo-1"),
    };

    let ctx = RequestContext::new();

    println!("├─ Method: {}", request.method);
    println!("├─ Request ID: {:?}", request.id);

    // Process request through middleware stack
    let (_processed_request, processed_ctx) = stack.process_request(request, ctx).await?;
    println!("├─ ✓ Request processed through middleware stack");

    // Create a mock response
    let response = JsonRpcResponse {
        jsonrpc: JsonRpcVersion,
        result: Some(json!({
            "tools": [
                {"name": "calculator", "description": "Performs calculations"},
                {"name": "file_reader", "description": "Reads files"}
            ]
        })),
        error: None,
        id: Some(MessageId::from("demo-1")),
    };

    // Process response through middleware stack (adds security headers)
    let processed_response = stack.process_response(response, &processed_ctx).await?;
    println!("├─ ✓ Response processed through middleware stack");

    // Extract and display security headers
    if let Some(result) = &processed_response.result
        && let Some(security_headers) = result.get("_security_headers")
    {
        println!("\n🔐 Applied Security Headers:");

        // Display security headers in a readable format
        if let Some(headers_obj) = security_headers.as_object() {
            for (header_name, header_value) in headers_obj {
                println!(
                    "├─ {}: {}",
                    header_name,
                    header_value.as_str().unwrap_or("N/A")
                );
            }
        }

        println!(
            "└─ Total headers applied: {}",
            security_headers.as_object().map(|o| o.len()).unwrap_or(0)
        );
    }

    // Demonstrate configuration comparison
    println!("\n📊 Configuration Comparison:");

    // Show differences between configurations
    println!("┌─ Default vs Strict vs Relaxed:");
    println!("├─ CSP Strictness: Default < Relaxed < Strict");
    println!("├─ Frame Options: Default(DENY) = Strict(DENY) > Relaxed(SAMEORIGIN)");
    println!("├─ HSTS: Default(1yr) < Strict(2yr), Relaxed(disabled)");
    println!("└─ Cross-Origin Policies: Strict > Default > Relaxed");

    // Best practices
    println!("\n💡 Security Best Practices:");
    println!("├─ Use 'strict' configuration for production environments");
    println!("├─ Use 'relaxed' configuration only during development");
    println!("├─ Customize CSP based on your application's specific needs");
    println!("├─ Enable HSTS only when using HTTPS exclusively");
    println!("├─ Regularly review and update security policies");
    println!("└─ Test security headers with browser developer tools");

    // Performance impact
    println!("\n⚡ Performance Characteristics:");
    println!("├─ Minimal CPU overhead (header string concatenation only)");
    println!("├─ No memory allocations during request processing");
    println!("├─ Headers applied during response phase only");
    println!("├─ Compatible with all transport types (HTTP, WebSocket, etc.)");
    println!("└─ No impact on JSON-RPC protocol semantics");

    println!("\n✅ Security Headers Middleware Demo Complete!");
    println!("   Your MCP server is now protected with defense-in-depth security!");

    Ok(())
}

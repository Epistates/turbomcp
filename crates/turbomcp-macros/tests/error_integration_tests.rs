//! Integration tests for error handling with McpError
//!
//! These tests verify that mcp_error! macro generates turbomcp_protocol::McpError
//! and works correctly at the macro layer.

use turbomcp_macros::*;
use turbomcp_protocol::{ErrorKind, McpError};

#[test]
fn test_mcp_error_generates_mcp_error() {
    let error = mcp_error!("Test error message");

    // Verify it's an McpError with Internal kind (default for handler errors)
    assert_eq!(error.kind, ErrorKind::Internal);
    assert_eq!(error.message, "Test error message");
}

#[test]
fn test_mcp_error_with_formatting() {
    let operation = "database_query";
    let code = 500;
    let error = mcp_error!("Operation {} failed with code {}", operation, code);

    assert_eq!(error.kind, ErrorKind::Internal);
    assert_eq!(
        error.message,
        "Operation database_query failed with code 500"
    );
}

#[test]
fn test_mcp_error_in_result_context() {
    fn failing_operation() -> Result<String, McpError> {
        Err(mcp_error!("Simulated failure"))
    }

    let result = failing_operation();
    assert!(result.is_err());

    if let Err(error) = result {
        assert_eq!(error.kind, ErrorKind::Internal);
        assert_eq!(error.message, "Simulated failure");
    }
}

#[test]
fn test_error_properties_preserved() {
    let error = mcp_error!("Test error");

    // Test error properties
    assert!(!error.is_retryable()); // Internal errors are not retryable
    assert!(!error.is_temporary());

    // Test HTTP status code mapping
    assert_eq!(error.http_status(), 500); // Internal errors map to 500

    // Test JSON-RPC error code mapping
    assert_eq!(error.jsonrpc_code(), -32603); // Internal error code
}

#[test]
fn test_mcp_error_direct_creation() {
    // Test that we can create different error types directly
    let internal_error = McpError::internal("Internal failure");
    let validation_error = McpError::invalid_params("Invalid input");
    let not_found_error = McpError::resource_not_found("Resource missing");

    // Verify the error types are correct
    assert_eq!(internal_error.kind, ErrorKind::Internal);
    assert_eq!(validation_error.kind, ErrorKind::InvalidParams);
    assert_eq!(not_found_error.kind, ErrorKind::ResourceNotFound);

    // Verify messages are preserved
    assert_eq!(internal_error.message, "Internal failure");
    assert_eq!(validation_error.message, "Invalid input");
    assert!(not_found_error.message.contains("Resource missing"));
}

#[test]
fn test_macro_error_vs_direct_error() {
    // Create same error via macro and direct call
    let macro_error = mcp_error!("Test message");
    let direct_error = McpError::internal("Test message");

    // Both should have same kind and message
    assert_eq!(macro_error.kind, direct_error.kind);
    assert_eq!(macro_error.message, direct_error.message);
    assert_eq!(macro_error.kind, ErrorKind::Internal);
}

#[test]
fn test_error_context_information() {
    let error = mcp_error!("Context test");

    // Verify the error has proper structure
    // By default, context is None (lazy initialization)
    assert!(error.context.is_none());

    // Test with context
    let error_with_ctx = McpError::internal("test")
        .with_operation("test_op")
        .with_component("test_comp");

    let ctx = error_with_ctx.context.as_ref().unwrap();
    assert_eq!(ctx.operation, Some("test_op".to_string()));
    assert_eq!(ctx.component, Some("test_comp".to_string()));
}

#[test]
fn test_complex_formatting_scenarios() {
    // Test various formatting scenarios
    let simple = mcp_error!("Simple message");
    assert_eq!(simple.message, "Simple message");

    let with_string = mcp_error!("Hello {}", "world");
    assert_eq!(with_string.message, "Hello world");

    let with_number = mcp_error!("Code: {}", 404);
    assert_eq!(with_number.message, "Code: 404");

    let with_multiple = mcp_error!("User {} has {} items", "alice", 5);
    assert_eq!(with_multiple.message, "User alice has 5 items");
}

#[test]
fn test_error_jsonrpc_codes() {
    // Test various error codes
    assert_eq!(McpError::parse_error("x").jsonrpc_code(), -32700);
    assert_eq!(McpError::invalid_request("x").jsonrpc_code(), -32600);
    assert_eq!(McpError::method_not_found("x").jsonrpc_code(), -32601);
    assert_eq!(McpError::invalid_params("x").jsonrpc_code(), -32602);
    assert_eq!(McpError::internal("x").jsonrpc_code(), -32603);
    assert_eq!(McpError::tool_not_found("x").jsonrpc_code(), -32001);
    assert_eq!(McpError::resource_not_found("x").jsonrpc_code(), -32004);
}

#[test]
fn test_error_http_status() {
    // Test HTTP status code mappings
    assert_eq!(McpError::invalid_params("x").http_status(), 400);
    assert_eq!(McpError::authentication("x").http_status(), 401);
    assert_eq!(McpError::permission_denied("x").http_status(), 403);
    assert_eq!(McpError::tool_not_found("x").http_status(), 404);
    assert_eq!(McpError::timeout("x").http_status(), 408);
    assert_eq!(McpError::rate_limited("x").http_status(), 429);
    assert_eq!(McpError::internal("x").http_status(), 500);
    assert_eq!(McpError::unavailable("x").http_status(), 503);
}

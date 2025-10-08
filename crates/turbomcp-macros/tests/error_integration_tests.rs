//! Integration tests for error handling after circular dependency fix
//!
//! These tests verify that mcp_error! macro now generates turbomcp_protocol::Error
//! and works correctly at the macro layer without creating circular dependencies.

use turbomcp_macros::*;
use turbomcp_protocol::{Error as CoreError, ErrorKind};

#[test]
fn test_mcp_error_generates_core_error() {
    let error = mcp_error!("Test error message");

    // Verify it's a core Error
    assert_eq!(error.kind, ErrorKind::Handler);
    assert_eq!(error.message, "Test error message");
}

#[test]
fn test_mcp_error_with_formatting() {
    let operation = "database_query";
    let code = 500;
    let error = mcp_error!("Operation {} failed with code {}", operation, code);

    assert_eq!(error.kind, ErrorKind::Handler);
    assert_eq!(
        error.message,
        "Operation database_query failed with code 500"
    );
}

#[test]
fn test_mcp_error_in_result_context() {
    fn failing_operation() -> Result<String, Box<CoreError>> {
        Err(mcp_error!("Simulated failure"))
    }

    let result = failing_operation();
    assert!(result.is_err());

    if let Err(error) = result {
        assert_eq!(error.kind, ErrorKind::Handler);
        assert_eq!(error.message, "Simulated failure");
    }
}

#[test]
fn test_error_properties_preserved() {
    let error = mcp_error!("Test error");

    // Test error properties
    assert!(!error.is_retryable()); // Handler errors are not retryable by default
    assert!(!error.is_temporary());

    // Test HTTP status code mapping
    assert_eq!(error.http_status_code(), 500); // Handler errors map to 500

    // Test JSON-RPC error code mapping
    assert_eq!(error.jsonrpc_error_code(), -32011); // Handler error code
}

#[test]
fn test_core_error_direct_creation() {
    // Test that we can create different error types from core directly
    let handler_error = CoreError::handler("Handler failed");
    let validation_error = CoreError::validation("Invalid input");
    let not_found_error = CoreError::not_found("Resource missing");

    // Verify the error types are correct
    assert_eq!(handler_error.kind, ErrorKind::Handler);
    assert_eq!(validation_error.kind, ErrorKind::Validation);
    assert_eq!(not_found_error.kind, ErrorKind::NotFound);

    // Verify messages are preserved
    assert_eq!(handler_error.message, "Handler failed");
    assert_eq!(validation_error.message, "Invalid input");
    assert_eq!(not_found_error.message, "Resource missing");
}

#[test]
fn test_macro_error_vs_direct_error() {
    // Create same error via macro and direct call
    let macro_error = mcp_error!("Test message");
    let direct_error = CoreError::handler("Test message");

    // Both should have same kind and message
    assert_eq!(macro_error.kind, direct_error.kind);
    assert_eq!(macro_error.message, direct_error.message);
    assert_eq!(macro_error.kind, ErrorKind::Handler);
}

#[test]
fn test_error_context_information() {
    let error = mcp_error!("Context test");

    // Verify the error has proper context structure
    // Note: We can't import chrono in macros crate due to dev-dependency restrictions
    // But we can verify the timestamp exists and is not default
    assert!(!error.context.timestamp.to_string().is_empty());
    assert_eq!(error.context.operation, None); // Default has no operation
    assert_eq!(error.context.component, None); // Default has no component

    // Verify it has a unique ID (not nil UUID)
    assert_ne!(error.id.to_string(), "00000000-0000-0000-0000-000000000000");
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

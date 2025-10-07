//! Comprehensive error handling tests for maximum coverage

use serde_json::json;
use turbomcp_core::error::*;
use turbomcp_core::mcp_error;
use uuid::Uuid;

// ============================================================================
// Error Creation Tests
// ============================================================================

#[test]
fn test_error_new() {
    let error = Error::new(ErrorKind::Validation, "Test message");
    assert_eq!(error.kind, ErrorKind::Validation);
    assert_eq!(error.message, "Test message");
    assert!(error.id != Uuid::nil());
    assert!(error.source.is_none());
    assert!(error.context.timestamp <= chrono::Utc::now());
}

#[test]
fn test_all_error_kind_constructors() {
    let validation = Error::validation("validation error");
    assert_eq!(validation.kind, ErrorKind::Validation);
    assert_eq!(validation.message, "validation error");

    let authentication = Error::authentication("auth error");
    assert_eq!(authentication.kind, ErrorKind::Authentication);
    assert_eq!(authentication.message, "auth error");

    let not_found = Error::not_found("not found error");
    assert_eq!(not_found.kind, ErrorKind::NotFound);
    assert_eq!(not_found.message, "not found error");

    let permission_denied = Error::permission_denied("permission error");
    assert_eq!(permission_denied.kind, ErrorKind::PermissionDenied);
    assert_eq!(permission_denied.message, "permission error");

    let bad_request = Error::bad_request("bad request error");
    assert_eq!(bad_request.kind, ErrorKind::BadRequest);
    assert_eq!(bad_request.message, "bad request error");

    let internal = Error::internal("internal error");
    assert_eq!(internal.kind, ErrorKind::Internal);
    assert_eq!(internal.message, "internal error");

    let transport = Error::transport("transport error");
    assert_eq!(transport.kind, ErrorKind::Transport);
    assert_eq!(transport.message, "transport error");

    let serialization = Error::serialization("serialization error");
    assert_eq!(serialization.kind, ErrorKind::Serialization);
    assert_eq!(serialization.message, "serialization error");

    let protocol = Error::protocol("protocol error");
    assert_eq!(protocol.kind, ErrorKind::Protocol);
    assert_eq!(protocol.message, "protocol error");

    let timeout = Error::timeout("timeout error");
    assert_eq!(timeout.kind, ErrorKind::Timeout);
    assert_eq!(timeout.message, "timeout error");

    let unavailable = Error::unavailable("unavailable error");
    assert_eq!(unavailable.kind, ErrorKind::Unavailable);
    assert_eq!(unavailable.message, "unavailable error");

    let rate_limited = Error::rate_limited("rate limited error");
    assert_eq!(rate_limited.kind, ErrorKind::RateLimited);
    assert_eq!(rate_limited.message, "rate limited error");

    let configuration = Error::configuration("config error");
    assert_eq!(configuration.kind, ErrorKind::Configuration);
    assert_eq!(configuration.message, "config error");

    let external_service = Error::external_service("external error");
    assert_eq!(external_service.kind, ErrorKind::ExternalService);
    assert_eq!(external_service.message, "external error");

    let cancelled = Error::cancelled("cancelled error");
    assert_eq!(cancelled.kind, ErrorKind::Cancelled);
    assert_eq!(cancelled.message, "cancelled error");
}

// ============================================================================
// Error Context Tests
// ============================================================================

#[test]
fn test_error_context_default() {
    let context = ErrorContext::default();
    assert!(context.operation.is_none());
    assert!(context.component.is_none());
    assert!(context.request_id.is_none());
    assert!(context.user_id.is_none());
    assert!(context.metadata.is_empty());
    assert!(context.retry_info.is_none());
}

#[test]
fn test_error_with_operation() {
    let error = Error::internal("test").with_operation("database_query");
    assert_eq!(error.context.operation, Some("database_query".to_string()));
}

#[test]
fn test_error_with_component() {
    let error = Error::internal("test").with_component("user_service");
    assert_eq!(error.context.component, Some("user_service".to_string()));
}

#[test]
fn test_error_with_request_id() {
    let error = Error::internal("test").with_request_id("req-abc-123");
    assert_eq!(error.context.request_id, Some("req-abc-123".to_string()));
}

#[test]
fn test_error_with_user_id() {
    let error = Error::internal("test").with_user_id("user-456");
    assert_eq!(error.context.user_id, Some("user-456".to_string()));
}

#[test]
fn test_error_with_context_string() {
    let error = Error::internal("test").with_context("filename", "test.txt");
    assert_eq!(
        error.context.metadata.get("filename"),
        Some(&json!("test.txt"))
    );
}

#[test]
fn test_error_with_context_number() {
    let error = Error::internal("test").with_context("line_number", 42);
    assert_eq!(error.context.metadata.get("line_number"), Some(&json!(42)));
}

#[test]
fn test_error_with_context_bool() {
    let error = Error::internal("test").with_context("is_critical", true);
    assert_eq!(
        error.context.metadata.get("is_critical"),
        Some(&json!(true))
    );
}

#[test]
fn test_error_with_context_complex_json() {
    let complex_data = json!({
        "config": {
            "timeout": 5000,
            "retries": 3
        },
        "tags": ["urgent", "database"]
    });
    let error = Error::internal("test").with_context("details", complex_data.clone());
    assert_eq!(error.context.metadata.get("details"), Some(&complex_data));
}

#[test]
fn test_error_with_multiple_context() {
    let error = Error::internal("test")
        .with_context("key1", "value1")
        .with_context("key2", 123)
        .with_context("key3", json!({"nested": "value"}));

    assert_eq!(error.context.metadata.len(), 3);
    assert_eq!(error.context.metadata.get("key1"), Some(&json!("value1")));
    assert_eq!(error.context.metadata.get("key2"), Some(&json!(123)));
    assert_eq!(
        error.context.metadata.get("key3"),
        Some(&json!({"nested": "value"}))
    );
}

#[test]
fn test_error_chaining_context() {
    let error = Error::validation("Invalid input")
        .with_operation("user_registration")
        .with_component("validation_service")
        .with_request_id("req-123")
        .with_user_id("user-456")
        .with_context("field", "email")
        .with_context("value", "invalid-email");

    assert_eq!(
        error.context.operation,
        Some("user_registration".to_string())
    );
    assert_eq!(
        error.context.component,
        Some("validation_service".to_string())
    );
    assert_eq!(error.context.request_id, Some("req-123".to_string()));
    assert_eq!(error.context.user_id, Some("user-456".to_string()));
    assert_eq!(error.context.metadata.len(), 2);
}

// ============================================================================
// Retry Info Tests
// ============================================================================

#[test]
fn test_retry_info_creation() {
    let retry_info = RetryInfo {
        attempts: 2,
        max_attempts: 5,
        retry_after_ms: Some(1000),
    };

    assert_eq!(retry_info.attempts, 2);
    assert_eq!(retry_info.max_attempts, 5);
    assert_eq!(retry_info.retry_after_ms, Some(1000));
}

#[test]
fn test_error_with_retry_info() {
    let retry_info = RetryInfo {
        attempts: 3,
        max_attempts: 5,
        retry_after_ms: Some(2000),
    };

    let error = Error::timeout("Request timed out").with_retry_info(retry_info.clone());

    assert!(error.context.retry_info.is_some());
    let stored_retry = error.context.retry_info.unwrap();
    assert_eq!(stored_retry.attempts, 3);
    assert_eq!(stored_retry.max_attempts, 5);
    assert_eq!(stored_retry.retry_after_ms, Some(2000));
}

#[test]
fn test_retry_info_without_retry_after() {
    let retry_info = RetryInfo {
        attempts: 1,
        max_attempts: 3,
        retry_after_ms: None,
    };

    let error = Error::unavailable("Service down").with_retry_info(retry_info);
    let stored_retry = error.context.retry_info.unwrap();
    assert!(stored_retry.retry_after_ms.is_none());
}

#[test]
fn test_retry_info_serialization() {
    let retry_info = RetryInfo {
        attempts: 4,
        max_attempts: 10,
        retry_after_ms: Some(5000),
    };

    let serialized = serde_json::to_string(&retry_info).unwrap();
    let deserialized: RetryInfo = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.attempts, 4);
    assert_eq!(deserialized.max_attempts, 10);
    assert_eq!(deserialized.retry_after_ms, Some(5000));
}

// ============================================================================
// Error Source/Chaining Tests
// ============================================================================

#[test]
fn test_error_with_source() {
    let source_error = Error::serialization("JSON parse error");
    let main_error = Error::internal("Failed to process request").with_source(source_error);

    assert!(main_error.source.is_some());
    let source = main_error.source.as_ref().unwrap();
    assert_eq!(source.kind, ErrorKind::Serialization);
    assert_eq!(source.message, "JSON parse error");
}

#[test]
fn test_error_source_chaining() {
    let root_error = Error::transport("Network connection lost");
    let middle_error = Error::external_service("API call failed").with_source(root_error);
    let top_error = Error::internal("Request processing failed").with_source(middle_error);

    assert!(top_error.source.is_some());
    let middle = top_error.source.as_ref().unwrap();
    assert_eq!(middle.kind, ErrorKind::ExternalService);

    assert!(middle.source.is_some());
    let root = middle.source.as_ref().unwrap();
    assert_eq!(root.kind, ErrorKind::Transport);
    assert_eq!(root.message, "Network connection lost");
}

#[test]
fn test_error_source_clone() {
    let source_error = Error::timeout("Operation timed out");
    let main_error = Error::internal("Processing failed").with_source(source_error);

    let cloned_error = main_error.clone();

    assert!(cloned_error.source.is_some());
    let cloned_source = cloned_error.source.as_ref().unwrap();
    assert_eq!(cloned_source.kind, ErrorKind::Timeout);
    assert_eq!(cloned_source.message, "Operation timed out");
}

// ============================================================================
// Error Kind Tests
// ============================================================================

#[test]
fn test_error_kind_equality() {
    assert_eq!(ErrorKind::Validation, ErrorKind::Validation);
    assert_ne!(ErrorKind::Validation, ErrorKind::Authentication);
}

#[test]
fn test_error_kind_hash() {
    use std::collections::HashSet;

    let mut kinds = HashSet::new();
    kinds.insert(ErrorKind::Validation);
    kinds.insert(ErrorKind::Authentication);
    kinds.insert(ErrorKind::NotFound);

    assert!(kinds.contains(&ErrorKind::Validation));
    assert!(!kinds.contains(&ErrorKind::Internal));
    assert_eq!(kinds.len(), 3);
}

#[test]
fn test_error_kind_copy_clone() {
    let original = ErrorKind::Timeout;
    let copied = original; // Tests Copy
    let cloned = original; // Tests Clone

    assert_eq!(original, copied);
    assert_eq!(original, cloned);
    assert_eq!(copied, cloned);
}

#[test]
fn test_error_kind_description() {
    assert_eq!(
        ErrorKind::Validation.description(),
        "Input validation failed"
    );
    assert_eq!(
        ErrorKind::Authentication.description(),
        "Authentication failed"
    );
    assert_eq!(ErrorKind::NotFound.description(), "Resource not found");
    assert_eq!(
        ErrorKind::PermissionDenied.description(),
        "Permission denied"
    );
    assert_eq!(ErrorKind::BadRequest.description(), "Bad request");
    assert_eq!(ErrorKind::Internal.description(), "Internal server error");
    assert_eq!(ErrorKind::Transport.description(), "Transport error");
    assert_eq!(
        ErrorKind::Serialization.description(),
        "Serialization error"
    );
    assert_eq!(ErrorKind::Protocol.description(), "Protocol error");
    assert_eq!(ErrorKind::Timeout.description(), "Operation timed out");
    assert_eq!(ErrorKind::Unavailable.description(), "Service unavailable");
    assert_eq!(ErrorKind::RateLimited.description(), "Rate limit exceeded");
    assert_eq!(
        ErrorKind::Configuration.description(),
        "Configuration error"
    );
    assert_eq!(
        ErrorKind::ExternalService.description(),
        "External service error"
    );
    assert_eq!(ErrorKind::Cancelled.description(), "Operation cancelled");
}

#[test]
fn test_error_kind_display() {
    assert_eq!(
        format!("{}", ErrorKind::Validation),
        "Input validation failed"
    );
    assert_eq!(
        format!("{}", ErrorKind::Authentication),
        "Authentication failed"
    );
    assert_eq!(format!("{}", ErrorKind::NotFound), "Resource not found");
    assert_eq!(format!("{}", ErrorKind::Internal), "Internal server error");
}

#[test]
fn test_error_kind_debug() {
    let debug_str = format!("{:?}", ErrorKind::Validation);
    assert!(debug_str.contains("Validation"));
}

// ============================================================================
// Error Behavior Tests
// ============================================================================

#[test]
fn test_is_retryable() {
    // Retryable errors
    assert!(Error::timeout("timeout").is_retryable());
    assert!(Error::unavailable("unavailable").is_retryable());
    assert!(Error::transport("transport").is_retryable());
    assert!(Error::external_service("external").is_retryable());
    assert!(Error::rate_limited("rate limited").is_retryable());

    // Non-retryable errors
    assert!(!Error::validation("validation").is_retryable());
    assert!(!Error::authentication("auth").is_retryable());
    assert!(!Error::not_found("not found").is_retryable());
    assert!(!Error::permission_denied("permission").is_retryable());
    assert!(!Error::bad_request("bad request").is_retryable());
    assert!(!Error::internal("internal").is_retryable());
    assert!(!Error::serialization("serialization").is_retryable());
    assert!(!Error::protocol("protocol").is_retryable());
    assert!(!Error::configuration("config").is_retryable());
    assert!(!Error::cancelled("cancelled").is_retryable());
}

#[test]
fn test_is_temporary() {
    // Temporary errors
    assert!(Error::timeout("timeout").is_temporary());
    assert!(Error::unavailable("unavailable").is_temporary());
    assert!(Error::rate_limited("rate limited").is_temporary());
    assert!(Error::external_service("external").is_temporary());

    // Permanent errors
    assert!(!Error::validation("validation").is_temporary());
    assert!(!Error::authentication("auth").is_temporary());
    assert!(!Error::not_found("not found").is_temporary());
    assert!(!Error::permission_denied("permission").is_temporary());
    assert!(!Error::bad_request("bad request").is_temporary());
    assert!(!Error::internal("internal").is_temporary());
    assert!(!Error::transport("transport").is_temporary());
    assert!(!Error::serialization("serialization").is_temporary());
    assert!(!Error::protocol("protocol").is_temporary());
    assert!(!Error::configuration("config").is_temporary());
    assert!(!Error::cancelled("cancelled").is_temporary());
}

// ============================================================================
// HTTP Status Code Tests
// ============================================================================

#[test]
fn test_http_status_codes() {
    assert_eq!(Error::validation("test").http_status_code(), 400);
    assert_eq!(Error::bad_request("test").http_status_code(), 400);
    assert_eq!(Error::authentication("test").http_status_code(), 401);
    assert_eq!(Error::permission_denied("test").http_status_code(), 403);
    assert_eq!(Error::not_found("test").http_status_code(), 404);
    assert_eq!(Error::timeout("test").http_status_code(), 408);
    assert_eq!(Error::rate_limited("test").http_status_code(), 429);
    assert_eq!(Error::cancelled("test").http_status_code(), 499);

    // 500 status codes
    assert_eq!(Error::internal("test").http_status_code(), 500);
    assert_eq!(Error::configuration("test").http_status_code(), 500);
    assert_eq!(Error::serialization("test").http_status_code(), 500);
    assert_eq!(Error::protocol("test").http_status_code(), 500);

    // 503 status codes
    assert_eq!(Error::transport("test").http_status_code(), 503);
    assert_eq!(Error::external_service("test").http_status_code(), 503);
    assert_eq!(Error::unavailable("test").http_status_code(), 503);
}

// ============================================================================
// JSON-RPC Error Code Tests
// ============================================================================

#[test]
fn test_jsonrpc_error_codes() {
    // Standard JSON-RPC error codes
    assert_eq!(Error::bad_request("test").jsonrpc_error_code(), -32600);
    assert_eq!(Error::protocol("test").jsonrpc_error_code(), -32601);
    assert_eq!(Error::internal("test").jsonrpc_error_code(), -32603);

    // -32602 (Invalid params) - MCP spec requires this for validation errors
    assert_eq!(Error::validation("test").jsonrpc_error_code(), -32602);
    assert_eq!(Error::invalid_params("test").jsonrpc_error_code(), -32602);
    assert_eq!(Error::serialization("test").jsonrpc_error_code(), -32602);

    // Custom codes
    assert_eq!(Error::not_found("test").jsonrpc_error_code(), -32001);
    assert_eq!(Error::authentication("test").jsonrpc_error_code(), -32002);
    assert_eq!(
        Error::permission_denied("test").jsonrpc_error_code(),
        -32003
    );
    assert_eq!(Error::timeout("test").jsonrpc_error_code(), -32004);
    assert_eq!(Error::unavailable("test").jsonrpc_error_code(), -32005);
    assert_eq!(Error::rate_limited("test").jsonrpc_error_code(), -32006);
    assert_eq!(Error::transport("test").jsonrpc_error_code(), -32007);
    assert_eq!(Error::configuration("test").jsonrpc_error_code(), -32008);
    assert_eq!(Error::external_service("test").jsonrpc_error_code(), -32009);
    assert_eq!(Error::cancelled("test").jsonrpc_error_code(), -32010);
}

// ============================================================================
// Display and Debug Tests
// ============================================================================

#[test]
fn test_error_display_minimal() {
    let error = Error::internal("Something went wrong");
    let display_str = format!("{error}");
    assert_eq!(display_str, "Something went wrong");
}

#[test]
fn test_error_display_with_operation() {
    let error = Error::internal("Something went wrong").with_operation("database_query");
    let display_str = format!("{error}");
    assert!(display_str.contains("Something went wrong"));
    assert!(display_str.contains("(operation: database_query)"));
}

#[test]
fn test_error_display_with_component() {
    let error = Error::internal("Something went wrong").with_component("user_service");
    let display_str = format!("{error}");
    assert!(display_str.contains("Something went wrong"));
    assert!(display_str.contains("(component: user_service)"));
}

#[test]
fn test_error_display_with_request_id() {
    let error = Error::internal("Something went wrong").with_request_id("req-123");
    let display_str = format!("{error}");
    assert!(display_str.contains("Something went wrong"));
    assert!(display_str.contains("(request_id: req-123)"));
}

#[test]
fn test_error_display_with_all_context() {
    let error = Error::internal("Something went wrong")
        .with_operation("database_query")
        .with_component("user_service")
        .with_request_id("req-123");

    let display_str = format!("{error}");
    assert!(display_str.contains("Something went wrong"));
    assert!(display_str.contains("(operation: database_query)"));
    assert!(display_str.contains("(component: user_service)"));
    assert!(display_str.contains("(request_id: req-123)"));
}

#[test]
fn test_error_debug() {
    let error = Error::validation("Invalid input").with_context("field", "email");
    let debug_str = format!("{error:?}");
    assert!(debug_str.contains("Error"));
    assert!(debug_str.contains("Validation"));
    assert!(debug_str.contains("Invalid input"));
}

// ============================================================================
// Serialization Tests
// ============================================================================

#[test]
fn test_error_serialization() {
    let error = Error::validation("Invalid input")
        .with_operation("user_registration")
        .with_context("field", "email");

    let serialized = serde_json::to_string(&error).unwrap();
    assert!(serialized.contains("validation"));
    assert!(serialized.contains("Invalid input"));
    assert!(serialized.contains("user_registration"));
    assert!(serialized.contains("email"));
}

#[test]
fn test_error_deserialization() {
    let json = r#"{
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "kind": "validation",
        "message": "Invalid input",
        "context": {
            "operation": "user_registration",
            "component": null,
            "request_id": null,
            "user_id": null,
            "metadata": {"field": "email"},
            "timestamp": "2023-01-01T00:00:00Z",
            "retry_info": null
        }
    }"#;

    let error: Error = serde_json::from_str(json).unwrap();
    assert_eq!(error.kind, ErrorKind::Validation);
    assert_eq!(error.message, "Invalid input");
    assert_eq!(
        error.context.operation,
        Some("user_registration".to_string())
    );
    assert_eq!(error.context.metadata.get("field"), Some(&json!("email")));
}

#[test]
fn test_error_context_serialization() {
    let mut context = ErrorContext {
        operation: Some("test_op".to_string()),
        component: Some("test_component".to_string()),
        request_id: Some("req-123".to_string()),
        user_id: Some("user-456".to_string()),
        retry_info: Some(RetryInfo {
            attempts: 2,
            max_attempts: 5,
            retry_after_ms: Some(1000),
        }),
        ..Default::default()
    };
    context.metadata.insert("key1".to_string(), json!("value1"));

    let serialized = serde_json::to_string(&context).unwrap();
    let deserialized: ErrorContext = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.operation, Some("test_op".to_string()));
    assert_eq!(deserialized.component, Some("test_component".to_string()));
    assert_eq!(deserialized.request_id, Some("req-123".to_string()));
    assert_eq!(deserialized.user_id, Some("user-456".to_string()));
    assert_eq!(deserialized.metadata.get("key1"), Some(&json!("value1")));
    assert!(deserialized.retry_info.is_some());
}

#[test]
fn test_error_kind_serialization() {
    let kinds = vec![
        ErrorKind::Validation,
        ErrorKind::Authentication,
        ErrorKind::NotFound,
        ErrorKind::PermissionDenied,
        ErrorKind::BadRequest,
        ErrorKind::Internal,
        ErrorKind::Transport,
        ErrorKind::Serialization,
        ErrorKind::Protocol,
        ErrorKind::Timeout,
        ErrorKind::Unavailable,
        ErrorKind::RateLimited,
        ErrorKind::Configuration,
        ErrorKind::ExternalService,
        ErrorKind::Cancelled,
    ];

    for kind in kinds {
        let serialized = serde_json::to_string(&kind).unwrap();
        let deserialized: ErrorKind = serde_json::from_str(&serialized).unwrap();
        assert_eq!(kind, deserialized);
    }
}

// ============================================================================
// Error Extension Trait Tests
// ============================================================================

#[test]
fn test_error_ext_with_mcp_error() {
    let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
    let result: std::result::Result<(), _> = Err(io_error);

    let mcp_result = result.with_mcp_error(ErrorKind::NotFound, "Failed to read config file");

    assert!(mcp_result.is_err());
    let error = mcp_result.unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert!(error.message.contains("Failed to read config file"));
    assert!(error.message.contains("File not found"));
    assert!(error.context.metadata.contains_key("source_error"));
}

#[test]
fn test_error_ext_with_internal_error() {
    let parse_error = "123abc".parse::<i32>();
    let mcp_result = parse_error.with_internal_error("Failed to parse configuration");

    assert!(mcp_result.is_err());
    let error = mcp_result.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Internal);
    assert!(error.message.contains("Failed to parse configuration"));
}

#[test]
fn test_error_ext_success_passthrough() {
    let ok_result: std::result::Result<i32, std::io::Error> = Ok(42);
    let mcp_result = ok_result.with_mcp_error(ErrorKind::Internal, "This shouldn't happen");

    assert!(mcp_result.is_ok());
    assert_eq!(mcp_result.unwrap(), 42);
}

// ============================================================================
// From Trait Implementation Tests
// ============================================================================

#[test]
fn test_from_serde_json_error() {
    let invalid_json = r#"{"invalid": json syntax"#;
    let parse_result: std::result::Result<serde_json::Value, _> =
        serde_json::from_str(invalid_json);

    assert!(parse_result.is_err());
    let serde_error = parse_result.unwrap_err();
    let mcp_error: Box<Error> = serde_error.into();

    assert_eq!(mcp_error.kind, ErrorKind::Serialization);
    assert!(mcp_error.message.contains("JSON serialization error"));
}

#[test]
fn test_from_io_error() {
    let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Access denied");
    let mcp_error: Box<Error> = io_error.into();

    assert_eq!(mcp_error.kind, ErrorKind::Transport);
    assert!(mcp_error.message.contains("IO error"));
    assert!(mcp_error.message.contains("Access denied"));
}

// ============================================================================
// Error Macro Tests
// ============================================================================

#[test]
fn test_mcp_error_macro_simple() {
    let error = mcp_error!(ErrorKind::Validation, "Invalid input");
    assert_eq!(error.kind, ErrorKind::Validation);
    assert_eq!(error.message, "Invalid input");
    assert!(error.context.metadata.is_empty());
}

#[test]
fn test_mcp_error_macro_with_single_context() {
    let error = mcp_error!(ErrorKind::Authentication, "Login failed", "username" => "alice");
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert_eq!(error.message, "Login failed");
    assert_eq!(error.context.metadata.len(), 1);
    assert_eq!(
        error.context.metadata.get("username"),
        Some(&json!("alice"))
    );
}

#[test]
fn test_mcp_error_macro_with_multiple_context() {
    let error = mcp_error!(
        ErrorKind::RateLimited,
        "Too many requests",
        "user_id" => "user123",
        "attempts" => 5,
        "window" => "1m"
    );

    assert_eq!(error.kind, ErrorKind::RateLimited);
    assert_eq!(error.message, "Too many requests");
    assert_eq!(error.context.metadata.len(), 3);
    assert_eq!(
        error.context.metadata.get("user_id"),
        Some(&json!("user123"))
    );
    assert_eq!(error.context.metadata.get("attempts"), Some(&json!(5)));
    assert_eq!(error.context.metadata.get("window"), Some(&json!("1m")));
}

#[test]
fn test_mcp_error_macro_with_complex_values() {
    let config_object = json!({
        "timeout": 5000,
        "retries": 3,
        "endpoints": ["api1.com", "api2.com"]
    });

    let error = mcp_error!(
        ErrorKind::Configuration,
        "Invalid configuration",
        "config" => config_object,
        "is_valid" => false
    );

    assert_eq!(error.kind, ErrorKind::Configuration);
    assert_eq!(error.context.metadata.len(), 2);
    assert!(error.context.metadata.get("config").unwrap().is_object());
    assert_eq!(error.context.metadata.get("is_valid"), Some(&json!(false)));
}

// ============================================================================
// Error Clone Tests
// ============================================================================

#[test]
fn test_error_clone_basic() {
    let original = Error::timeout("Request timed out")
        .with_operation("api_call")
        .with_context("url", "https://api.example.com");

    let cloned = original.clone();

    assert_eq!(cloned.kind, original.kind);
    assert_eq!(cloned.message, original.message);
    assert_eq!(cloned.context.operation, original.context.operation);
    assert_eq!(cloned.context.metadata, original.context.metadata);

    // IDs should be the same in clones
    assert_eq!(cloned.id, original.id);
}

#[test]
fn test_error_clone_with_source() {
    let source = Error::transport("Network failure");
    let original = Error::external_service("API unavailable").with_source(source);

    let cloned = original.clone();

    assert_eq!(cloned.kind, original.kind);
    assert!(cloned.source.is_some());

    let cloned_source = cloned.source.as_ref().unwrap();
    let original_source = original.source.as_ref().unwrap();
    assert_eq!(cloned_source.kind, original_source.kind);
    assert_eq!(cloned_source.message, original_source.message);
}

// ============================================================================
// Error Context Clone Tests
// ============================================================================

#[test]
fn test_error_context_clone() {
    let mut original = ErrorContext {
        operation: Some("test_op".to_string()),
        retry_info: Some(RetryInfo {
            attempts: 3,
            max_attempts: 5,
            retry_after_ms: Some(2000),
        }),
        ..Default::default()
    };
    original
        .metadata
        .insert("key1".to_string(), json!("value1"));

    let cloned = original.clone();

    assert_eq!(cloned.operation, original.operation);
    assert_eq!(cloned.metadata, original.metadata);
    assert!(cloned.retry_info.is_some());

    let cloned_retry = cloned.retry_info.as_ref().unwrap();
    let original_retry = original.retry_info.as_ref().unwrap();
    assert_eq!(cloned_retry.attempts, original_retry.attempts);
    assert_eq!(cloned_retry.max_attempts, original_retry.max_attempts);
    assert_eq!(cloned_retry.retry_after_ms, original_retry.retry_after_ms);
}

// ============================================================================
// Edge Cases and Error Conditions
// ============================================================================

#[test]
fn test_empty_error_message() {
    let error = Error::internal("");
    assert_eq!(error.message, "");
    assert_eq!(format!("{error}"), "");
}

#[test]
fn test_very_long_error_message() {
    let long_message = "x".repeat(10000);
    let error = Error::validation(&long_message);
    assert_eq!(error.message, long_message);
}

#[test]
fn test_unicode_error_message() {
    let unicode_message = "Ошибка валидации: 测试错误 🚨";
    let error = Error::validation(unicode_message);
    assert_eq!(error.message, unicode_message);
}

#[test]
fn test_error_with_special_characters() {
    let special_message = r#"Error with "quotes" and \backslashes\ and newlines\n"#;
    let error = Error::internal(special_message);
    assert_eq!(error.message, special_message);
}

#[test]
fn test_deeply_nested_error_chain() {
    let mut error = Error::transport("Root cause");

    // Create a chain of 10 errors
    for i in 1..=10 {
        error = Error::internal(format!("Error level {i}")).with_source(error);
    }

    // Verify the chain exists
    let mut current = &error;
    let mut depth = 0;
    while let Some(source) = &current.source {
        depth += 1;
        current = source;
    }

    assert_eq!(depth, 10);
    assert_eq!(current.message, "Root cause");
    assert_eq!(current.kind, ErrorKind::Transport);
}

#[test]
fn test_error_context_large_metadata() {
    let mut error = Error::internal("Test error");

    // Add 100 context items
    for i in 0..100 {
        error = error.with_context(format!("key_{i}"), format!("value_{i}"));
    }

    assert_eq!(error.context.metadata.len(), 100);
    assert_eq!(error.context.metadata.get("key_0"), Some(&json!("value_0")));
    assert_eq!(
        error.context.metadata.get("key_99"),
        Some(&json!("value_99"))
    );
}

// ============================================================================
// Result Type Alias Tests
// ============================================================================

#[test]
#[allow(clippy::unnecessary_literal_unwrap)]
fn test_result_type_alias() {
    let success: Result<i32> = Ok(42);
    assert!(success.is_ok());
    assert_eq!(success.unwrap(), 42);

    let failure: Result<i32> = Err(Error::validation("Invalid input"));
    assert!(failure.is_err());
    let error = failure.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Validation);
}

// ============================================================================
// Debug Assertions Tests (when enabled)
// ============================================================================

#[test]
#[cfg(debug_assertions)]
fn test_backtrace_capture() {
    let error = Error::internal("Test error");
    // We can't easily test the backtrace content, but we can ensure it exists
    let debug_str = format!("{error:?}");
    // The backtrace field should be present in debug builds
    assert!(debug_str.contains("backtrace"));
}

#[test]
fn test_error_source_trait_implementation() {
    let error = Error::internal("Test error");
    let std_error: &dyn std::error::Error = &error;

    // The source method should return None as documented
    assert!(std_error.source().is_none());
}

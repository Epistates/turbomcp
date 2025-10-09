//! Comprehensive protocol validation tests
//!
//! Following the world-class pattern from tokio/axum, these tests are co-located
//! with the validation.rs implementation. This gives tests access to private items
//! and makes it clear which code is being tested.

use super::*; // Import validation module items (ProtocolValidator, ValidationResult, etc.)
use crate::jsonrpc::{
    JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcVersion,
};
use serde_json::{Value, json};

// ========== ProtocolValidator Constructor Tests ==========

#[test]
fn test_protocol_validator_new() {
    let validator = ProtocolValidator::new();
    // Should use default rules and non-strict mode

    // Test via default method as well
    let default_validator = ProtocolValidator::default();
    // Validators should be equivalent in behavior

    let valid_request = create_valid_request();
    assert!(validator.validate_request(&valid_request).is_valid());
    assert!(
        default_validator
            .validate_request(&valid_request)
            .is_valid()
    );
}

#[test]
fn test_protocol_validator_with_strict_mode() {
    let validator = ProtocolValidator::new().with_strict_mode();

    // Test that strict mode affects validation behavior
    let request = create_valid_request();
    let result = validator.validate_request(&request);
    assert!(result.is_valid()); // Should still validate valid requests
}

#[test]
fn test_protocol_validator_with_custom_rules() {
    let custom_rules = ValidationRules {
        max_string_length: 10, // Very small limit
        max_array_length: 2,
        max_object_depth: 3,
        ..Default::default()
    };

    let validator = ProtocolValidator::new().with_rules(custom_rules);

    // Test with long string that should fail
    let tool = create_tool_with_long_name();
    let result = validator.validate_tool(&tool);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.code == "TOOL_NAME_TOO_LONG"));
}

#[test]
fn test_protocol_validator_builder_pattern() {
    let custom_rules = ValidationRules::default();

    let validator = ProtocolValidator::new()
        .with_strict_mode()
        .with_rules(custom_rules);

    // Should work with chained configuration
    let request = create_valid_request();
    assert!(validator.validate_request(&request).is_valid());
}

// ========== ValidationRules Tests ==========

#[test]
fn test_validation_rules_default() {
    let rules = ValidationRules::default();

    // Test default values
    assert_eq!(rules.max_message_size, 10 * 1024 * 1024); // 10MB
    assert_eq!(rules.max_batch_size, 100);
    assert_eq!(rules.max_string_length, 1024 * 1024); // 1MB
    assert_eq!(rules.max_array_length, 10000);
    assert_eq!(rules.max_object_depth, 32);

    // Test regex patterns work
    assert!(rules.uri_regex().is_match("file://test.txt"));
    assert!(rules.uri_regex().is_match("https://example.com"));
    assert!(!rules.uri_regex().is_match("not-a-uri"));

    assert!(rules.method_name_regex().is_match("tools/list"));
    assert!(rules.method_name_regex().is_match("initialize"));
    assert!(!rules.method_name_regex().is_match("invalid-method!"));

    // Test required fields
    assert!(rules.required_fields.contains_key("request"));
    assert!(rules.required_fields.contains_key("response"));
    assert!(rules.required_fields.contains_key("notification"));
    assert!(rules.required_fields.contains_key("initialize"));
    assert!(rules.required_fields.contains_key("tool"));
    assert!(rules.required_fields.contains_key("prompt"));
    assert!(rules.required_fields.contains_key("resource"));
}

#[test]
fn test_validation_rules_required_fields_structure() {
    let rules = ValidationRules::default();

    let request_fields = &rules.required_fields["request"];
    assert!(request_fields.contains("jsonrpc"));
    assert!(request_fields.contains("method"));
    assert!(request_fields.contains("id"));
    assert_eq!(request_fields.len(), 3);

    let response_fields = &rules.required_fields["response"];
    assert!(response_fields.contains("jsonrpc"));
    assert!(response_fields.contains("id"));
    assert_eq!(response_fields.len(), 2);

    let notification_fields = &rules.required_fields["notification"];
    assert!(notification_fields.contains("jsonrpc"));
    assert!(notification_fields.contains("method"));
    assert_eq!(notification_fields.len(), 2);

    let initialize_fields = &rules.required_fields["initialize"];
    assert!(initialize_fields.contains("protocolVersion"));
    assert!(initialize_fields.contains("capabilities"));
    assert!(initialize_fields.contains("clientInfo"));
    assert_eq!(initialize_fields.len(), 3);

    let tool_fields = &rules.required_fields["tool"];
    assert!(tool_fields.contains("name"));
    assert!(tool_fields.contains("inputSchema"));
    assert_eq!(tool_fields.len(), 2);

    let prompt_fields = &rules.required_fields["prompt"];
    assert!(prompt_fields.contains("name"));
    assert_eq!(prompt_fields.len(), 1);

    let resource_fields = &rules.required_fields["resource"];
    assert!(resource_fields.contains("uri"));
    assert!(resource_fields.contains("name"));
    assert_eq!(resource_fields.len(), 2);
}

// ========== JSON-RPC Request Validation Tests ==========

#[test]
fn test_validate_request_valid() {
    let validator = ProtocolValidator::new();
    let request = create_valid_request();

    let result = validator.validate_request(&request);
    assert!(result.is_valid());
    assert!(!result.has_warnings());
}

#[test]
fn test_validate_request_empty_method() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_request();
    request.method = String::new();

    let result = validator.validate_request(&request);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "EMPTY_METHOD_NAME");
    assert_eq!(errors[0].field_path, Some("method".to_string()));
}

#[test]
fn test_validate_request_invalid_method_name() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_request();
    request.method = "invalid-method!@#".to_string();

    let result = validator.validate_request(&request);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "INVALID_METHOD_NAME");
    assert!(errors[0].message.contains("invalid-method!@#"));
}

#[test]
fn test_validate_request_with_initialize_params() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_request();
    request.method = "initialize".to_string();
    request.params = Some(json!({
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {
            "name": "test",
            "version": "1.0.0"
        }
    }));

    let result = validator.validate_request(&request);
    assert!(result.is_valid());
}

#[test]
fn test_validate_request_tools_list_with_unexpected_params() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_request();
    request.method = "tools/list".to_string();
    request.params = Some(json!({"unexpected": "param"}));

    let result = validator.validate_request(&request);
    assert!(result.is_valid()); // Should still be valid but with warnings
    assert!(result.has_warnings());

    let warnings = result.warnings();
    assert!(!warnings.is_empty());
    assert!(warnings.iter().any(|w| w.code == "UNEXPECTED_PARAMS"));
}

#[test]
fn test_validate_request_tools_call() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_request();
    request.method = "tools/call".to_string();
    request.params = Some(json!({
        "name": "test_tool",
        "arguments": {}
    }));

    let result = validator.validate_request(&request);
    assert!(result.is_valid());
}

#[test]
fn test_validate_request_unknown_method() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_request();
    request.method = "custom/method".to_string();
    request.params = Some(json!({"key": "value"}));

    let result = validator.validate_request(&request);
    assert!(result.is_valid());
}

// ========== JSON-RPC Response Validation Tests ==========

#[test]
fn test_validate_response_valid_result() {
    let validator = ProtocolValidator::new();
    let response = JsonRpcResponse::success(
        json!({"status": "success"}),
        RequestId::String("test".to_string()),
    );

    let result = validator.validate_response(&response);
    assert!(result.is_valid());
}

#[test]
fn test_validate_response_valid_error() {
    let validator = ProtocolValidator::new();
    let response = JsonRpcResponse::error_response(
        JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        },
        RequestId::String("test".to_string()),
    );

    let result = validator.validate_response(&response);
    assert!(result.is_valid()); // Valid structure with negative error code
}

#[test]
fn test_validate_response_both_result_and_error() {
    // Test that JSON with both result and error fields deserializes to Success variant
    // (serde's untagged enum picks the first matching variant - Success)
    // This validates that our type-safe design enforces single variant behavior
    let json_with_both = r#"{
        "jsonrpc": "2.0",
        "result": {"status": "success"},
        "error": {
            "code": -32601,
            "message": "Method not found"
        },
        "id": "test"
    }"#;

    let response = serde_json::from_str::<JsonRpcResponse>(json_with_both).unwrap();

    // Should parse as Success (first variant), ignoring the error field
    assert!(response.is_success());
    assert!(!response.is_error());
    assert!(response.result().is_some());
    assert!(response.error().is_none());
}

#[test]
fn test_validate_response_neither_result_nor_error() {
    // Test that JSON with neither result nor error fields fails to deserialize
    // This validates our type-safe design that requires exactly one of result or error
    let invalid_json = r#"{
        "jsonrpc": "2.0",
        "id": "test"
    }"#;

    let parse_result = serde_json::from_str::<JsonRpcResponse>(invalid_json);
    assert!(
        parse_result.is_err(),
        "Should fail to parse JSON with neither result nor error fields"
    );
}

#[test]
fn test_validate_response_positive_error_code() {
    let validator = ProtocolValidator::new();
    let response = JsonRpcResponse::error_response(
        JsonRpcError {
            code: 1,
            message: "Positive error code".to_string(),
            data: None,
        },
        RequestId::String("test".to_string()),
    );

    let result = validator.validate_response(&response);
    assert!(result.is_valid()); // Valid but with warnings
    assert!(result.has_warnings());

    let warnings = result.warnings();
    assert!(warnings.iter().any(|w| w.code == "POSITIVE_ERROR_CODE"));
}

#[test]
fn test_validate_response_empty_error_message() {
    let validator = ProtocolValidator::new();
    let response = JsonRpcResponse::error_response(
        JsonRpcError {
            code: -32001,
            message: String::new(),
            data: None,
        },
        RequestId::String("test".to_string()),
    );

    let result = validator.validate_response(&response);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "EMPTY_ERROR_MESSAGE"));
}

// ========== JSON-RPC Notification Validation Tests ==========

#[test]
fn test_validate_notification_valid() {
    let validator = ProtocolValidator::new();
    let notification = JsonRpcNotification {
        jsonrpc: JsonRpcVersion,
        method: "notifications/progress".to_string(),
        params: Some(json!({"progressToken": "abc123", "progress": 50})),
    };

    let result = validator.validate_notification(&notification);
    assert!(result.is_valid());
}

#[test]
fn test_validate_notification_empty_method() {
    let validator = ProtocolValidator::new();
    let notification = JsonRpcNotification {
        jsonrpc: JsonRpcVersion,
        method: String::new(),
        params: None,
    };

    let result = validator.validate_notification(&notification);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "EMPTY_METHOD_NAME"));
}

#[test]
fn test_validate_notification_invalid_method() {
    let validator = ProtocolValidator::new();
    let notification = JsonRpcNotification {
        jsonrpc: JsonRpcVersion,
        method: "invalid-method!".to_string(),
        params: None,
    };

    let result = validator.validate_notification(&notification);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "INVALID_METHOD_NAME"));
}

#[test]
fn test_validate_notification_with_params() {
    let validator = ProtocolValidator::new();
    let notification = JsonRpcNotification {
        jsonrpc: JsonRpcVersion,
        method: "notifications/message".to_string(),
        params: Some(json!({
            "level": "info",
            "message": "Test message",
            "data": {"key": "value"}
        })),
    };

    let result = validator.validate_notification(&notification);
    assert!(result.is_valid());
}

// ========== Tool Validation Tests ==========

#[test]
fn test_validate_tool_valid() {
    let validator = ProtocolValidator::new();
    let tool = create_valid_tool();

    let result = validator.validate_tool(&tool);
    assert!(result.is_valid());
}

#[test]
fn test_validate_tool_empty_name() {
    let validator = ProtocolValidator::new();
    let mut tool = create_valid_tool();
    tool.name = String::new();

    let result = validator.validate_tool(&tool);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "TOOL_EMPTY_NAME"));
    assert_eq!(errors[0].field_path, Some("name".to_string()));
}

#[test]
fn test_validate_tool_name_too_long() {
    let validator = ProtocolValidator::new();
    let mut tool = create_valid_tool();
    tool.name = "x".repeat(2_000_000); // Exceed default max

    let result = validator.validate_tool(&tool);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "TOOL_NAME_TOO_LONG"));
}

#[test]
fn test_validate_tool_non_object_schema() {
    let validator = ProtocolValidator::new();
    let mut tool = create_valid_tool();
    tool.input_schema.schema_type = "string".to_string();

    let result = validator.validate_tool(&tool);
    assert!(result.is_valid()); // Valid but with warnings
    assert!(result.has_warnings());

    let warnings = result.warnings();
    assert!(warnings.iter().any(|w| w.code == "NON_OBJECT_SCHEMA"));
}

// ========== Prompt Validation Tests ==========

#[test]
fn test_validate_prompt_valid() {
    let validator = ProtocolValidator::new();
    let prompt = create_valid_prompt();

    let result = validator.validate_prompt(&prompt);
    assert!(result.is_valid());
}

#[test]
fn test_validate_prompt_empty_name() {
    let validator = ProtocolValidator::new();
    let mut prompt = create_valid_prompt();
    prompt.name = String::new();

    let result = validator.validate_prompt(&prompt);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "PROMPT_EMPTY_NAME"));
    assert_eq!(errors[0].field_path, Some("name".to_string()));
}

#[test]
fn test_validate_prompt_too_many_arguments() {
    let custom_rules = ValidationRules {
        max_array_length: 2,
        ..Default::default()
    };
    let validator = ProtocolValidator::new().with_rules(custom_rules);

    let mut prompt = create_valid_prompt();
    prompt.arguments = Some(vec![
        create_prompt_argument("arg1"),
        create_prompt_argument("arg2"),
        create_prompt_argument("arg3"), // This exceeds the limit
    ]);

    let result = validator.validate_prompt(&prompt);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "PROMPT_TOO_MANY_ARGS"));
}

#[test]
fn test_validate_prompt_with_no_arguments() {
    let validator = ProtocolValidator::new();
    let mut prompt = create_valid_prompt();
    prompt.arguments = None;

    let result = validator.validate_prompt(&prompt);
    assert!(result.is_valid());
}

// ========== Resource Validation Tests ==========

#[test]
fn test_validate_resource_valid() {
    let validator = ProtocolValidator::new();
    let resource = create_valid_resource();

    let result = validator.validate_resource(&resource);
    assert!(result.is_valid());
}

#[test]
fn test_validate_resource_invalid_uri() {
    let validator = ProtocolValidator::new();
    let mut resource = create_valid_resource();
    resource.uri = "not-a-valid-uri".to_string();

    let result = validator.validate_resource(&resource);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "RESOURCE_INVALID_URI"));
    assert!(errors[0].message.contains("not-a-valid-uri"));
    assert_eq!(errors[0].field_path, Some("uri".to_string()));
}

#[test]
fn test_validate_resource_empty_name() {
    let validator = ProtocolValidator::new();
    let mut resource = create_valid_resource();
    resource.name = String::new();

    let result = validator.validate_resource(&resource);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "RESOURCE_EMPTY_NAME"));
    assert_eq!(errors[0].field_path, Some("name".to_string()));
}

#[test]
fn test_validate_resource_various_uri_formats() {
    let validator = ProtocolValidator::new();

    let valid_uris = vec![
        "file://path/to/file.txt",
        "https://example.com/resource",
        "http://localhost:8080/api",
        "ftp://ftp.example.com/file.zip",
        "ssh://user@host:/path",
    ];

    for uri in valid_uris {
        let mut resource = create_valid_resource();
        resource.uri = uri.to_string();

        let result = validator.validate_resource(&resource);
        assert!(result.is_valid(), "URI should be valid: {uri}");
    }

    let invalid_uris = vec!["not-a-uri", "://missing-scheme", "scheme", ""];

    for uri in invalid_uris {
        let mut resource = create_valid_resource();
        resource.uri = uri.to_string();

        let result = validator.validate_resource(&resource);
        assert!(result.is_invalid(), "URI should be invalid: {uri}");
    }
}

// ========== Initialize Request Validation Tests ==========

#[test]
fn test_validate_initialize_request_valid() {
    let validator = ProtocolValidator::new();
    let request = create_valid_initialize_request();

    let result = validator.validate_initialize_request(&request);
    assert!(result.is_valid());
}

#[test]
fn test_validate_initialize_request_unsupported_version() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_initialize_request();
    request.protocol_version = "2020-01-01".to_string();

    let result = validator.validate_initialize_request(&request);
    assert!(result.is_valid()); // Valid but with warnings
    assert!(result.has_warnings());

    let warnings = result.warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.code == "UNSUPPORTED_PROTOCOL_VERSION")
    );
    assert!(warnings[0].message.contains("2020-01-01"));
    assert_eq!(warnings[0].field_path, Some("protocolVersion".to_string()));
}

#[test]
fn test_validate_initialize_request_empty_client_name() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_initialize_request();
    request.client_info.name = String::new();

    let result = validator.validate_initialize_request(&request);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "EMPTY_CLIENT_NAME"));
    assert_eq!(errors[0].field_path, Some("clientInfo.name".to_string()));
}

#[test]
fn test_validate_initialize_request_empty_client_version() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_initialize_request();
    request.client_info.version = String::new();

    let result = validator.validate_initialize_request(&request);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "EMPTY_CLIENT_VERSION"));
    assert_eq!(errors[0].field_path, Some("clientInfo.version".to_string()));
}

// ========== Value Structure Validation Tests ==========

#[test]
fn test_validate_deep_object_structure() {
    let custom_rules = ValidationRules {
        max_object_depth: 3,
        ..Default::default()
    };
    let validator = ProtocolValidator::new().with_rules(custom_rules);

    let deep_object = json!({
        "level1": {
            "level2": {
                "level3": {
                    "level4": "too_deep"
                }
            }
        }
    });

    let mut request = create_valid_request();
    request.method = "custom/deep".to_string();
    request.params = Some(deep_object);

    let result = validator.validate_request(&request);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "MAX_DEPTH_EXCEEDED"));
}

#[test]
fn test_validate_large_array() {
    let custom_rules = ValidationRules {
        max_array_length: 5,
        ..Default::default()
    };
    let validator = ProtocolValidator::new().with_rules(custom_rules);

    let large_array = json!([1, 2, 3, 4, 5, 6, 7]); // Exceeds limit

    let mut request = create_valid_request();
    request.method = "custom/array".to_string();
    request.params = Some(large_array);

    let result = validator.validate_request(&request);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "ARRAY_TOO_LONG"));
}

#[test]
fn test_validate_long_string() {
    let custom_rules = ValidationRules {
        max_string_length: 10,
        ..Default::default()
    };
    let validator = ProtocolValidator::new().with_rules(custom_rules);

    let long_string = "x".repeat(20);
    let params = json!({
        "text": long_string
    });

    let mut request = create_valid_request();
    request.method = "custom/string".to_string();
    request.params = Some(params);

    let result = validator.validate_request(&request);
    assert!(result.is_invalid());

    let errors = result.errors();
    assert!(errors.iter().any(|e| e.code == "STRING_TOO_LONG"));
}

#[test]
fn test_validate_complex_nested_structure() {
    let validator = ProtocolValidator::new();

    let complex_structure = json!({
        "object": {
            "array": [
                {"nested": "value1"},
                {"nested": "value2"},
                {"nested": ["item1", "item2", "item3"]}
            ],
            "string": "test",
            "number": 42,
            "boolean": true,
            "null": null
        }
    });

    let mut request = create_valid_request();
    request.method = "custom/complex".to_string();
    request.params = Some(complex_structure);

    let result = validator.validate_request(&request);
    assert!(result.is_valid());
}

// ========== ValidationResult Tests ==========

#[test]
fn test_validation_result_valid() {
    let result = ValidationResult::Valid;

    assert!(result.is_valid());
    assert!(!result.is_invalid());
    assert!(!result.has_warnings());
    assert!(result.warnings().is_empty());
    assert!(result.errors().is_empty());
}

#[test]
fn test_validation_result_valid_with_warnings() {
    let warnings = vec![
        ValidationWarning {
            code: "WARN1".to_string(),
            message: "Warning 1".to_string(),
            field_path: Some("field1".to_string()),
        },
        ValidationWarning {
            code: "WARN2".to_string(),
            message: "Warning 2".to_string(),
            field_path: None,
        },
    ];

    let result = ValidationResult::ValidWithWarnings(warnings.clone());

    assert!(result.is_valid());
    assert!(!result.is_invalid());
    assert!(result.has_warnings());
    assert_eq!(result.warnings(), &warnings);
    assert!(result.errors().is_empty());
}

#[test]
fn test_validation_result_invalid() {
    let errors = vec![
        ValidationError {
            code: "ERR1".to_string(),
            message: "Error 1".to_string(),
            field_path: Some("field1".to_string()),
        },
        ValidationError {
            code: "ERR2".to_string(),
            message: "Error 2".to_string(),
            field_path: None,
        },
    ];

    let result = ValidationResult::Invalid(errors.clone());

    assert!(!result.is_valid());
    assert!(result.is_invalid());
    assert!(!result.has_warnings());
    assert!(result.warnings().is_empty());
    assert_eq!(result.errors(), &errors);
}

// ========== ValidationError and ValidationWarning Tests ==========

#[test]
fn test_validation_error_structure() {
    let error = ValidationError {
        code: "TEST_ERROR".to_string(),
        message: "This is a test error".to_string(),
        field_path: Some("test.field".to_string()),
    };

    assert_eq!(error.code, "TEST_ERROR");
    assert_eq!(error.message, "This is a test error");
    assert_eq!(error.field_path, Some("test.field".to_string()));

    // Test PartialEq
    let same_error = ValidationError {
        code: "TEST_ERROR".to_string(),
        message: "This is a test error".to_string(),
        field_path: Some("test.field".to_string()),
    };

    assert_eq!(error, same_error);

    let different_error = ValidationError {
        code: "DIFFERENT_ERROR".to_string(),
        message: "This is a different error".to_string(),
        field_path: None,
    };

    assert_ne!(error, different_error);
}

#[test]
fn test_validation_warning_structure() {
    let warning = ValidationWarning {
        code: "TEST_WARNING".to_string(),
        message: "This is a test warning".to_string(),
        field_path: Some("test.field".to_string()),
    };

    assert_eq!(warning.code, "TEST_WARNING");
    assert_eq!(warning.message, "This is a test warning");
    assert_eq!(warning.field_path, Some("test.field".to_string()));

    // Test Clone
    let cloned_warning = warning.clone();
    assert_eq!(warning, cloned_warning);
}

// ========== Utilities Module Tests ==========

#[test]
fn test_utils_error_creation() {
    let error = utils::error("TEST_CODE", "Test message");

    assert_eq!(error.code, "TEST_CODE");
    assert_eq!(error.message, "Test message");
    assert_eq!(error.field_path, None);
}

#[test]
fn test_utils_warning_creation() {
    let warning = utils::warning("TEST_CODE", "Test message");

    assert_eq!(warning.code, "TEST_CODE");
    assert_eq!(warning.message, "Test message");
    assert_eq!(warning.field_path, None);
}

#[test]
fn test_utils_is_valid_uri() {
    // Valid URIs
    assert!(utils::is_valid_uri("file://test.txt"));
    assert!(utils::is_valid_uri("https://example.com"));
    assert!(utils::is_valid_uri("http://localhost"));
    assert!(utils::is_valid_uri("ftp://ftp.example.com"));
    assert!(utils::is_valid_uri("ssh://user@host"));

    // Invalid URIs
    assert!(!utils::is_valid_uri("not-a-uri"));
    assert!(!utils::is_valid_uri("://missing-scheme"));
    assert!(!utils::is_valid_uri("scheme"));
    assert!(!utils::is_valid_uri(""));
}

#[test]
fn test_utils_is_valid_method_name() {
    // Valid method names
    assert!(utils::is_valid_method_name("initialize"));
    assert!(utils::is_valid_method_name("tools/list"));
    assert!(utils::is_valid_method_name("tools/call"));
    assert!(utils::is_valid_method_name("custom_method"));
    assert!(utils::is_valid_method_name("namespace/method_name"));
    assert!(utils::is_valid_method_name("a"));
    assert!(utils::is_valid_method_name("test123"));

    // Invalid method names
    assert!(!utils::is_valid_method_name(""));
    assert!(!utils::is_valid_method_name("123invalid"));
    assert!(!utils::is_valid_method_name("invalid-method"));
    assert!(!utils::is_valid_method_name("invalid.method"));
    assert!(!utils::is_valid_method_name("invalid!method"));
    assert!(!utils::is_valid_method_name("invalid method"));
}

// ========== Edge Cases and Regression Tests ==========

#[test]
fn test_validation_with_null_params() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_request();
    request.params = Some(Value::Null);

    let result = validator.validate_request(&request);
    assert!(result.is_valid());
}

#[test]
fn test_validation_with_empty_object_params() {
    let validator = ProtocolValidator::new();
    let mut request = create_valid_request();
    request.params = Some(json!({}));

    let result = validator.validate_request(&request);
    assert!(result.is_valid());
}

#[test]
fn test_validation_context_path_handling() {
    let validator = ProtocolValidator::new();

    let nested_params = json!({
        "parent": {
            "child": {
                "grandchild": "value"
            }
        }
    });

    let mut request = create_valid_request();
    request.method = "custom/nested".to_string();
    request.params = Some(nested_params);

    let result = validator.validate_request(&request);
    assert!(result.is_valid());
}

#[test]
fn test_regex_patterns_compilation() {
    let rules = ValidationRules::default();

    // Test that regex patterns are properly compiled and don't panic
    assert!(rules.uri_regex().is_match("https://example.com"));
    assert!(rules.method_name_regex().is_match("valid_method"));

    // Test edge cases
    assert!(!rules.uri_regex().is_match(""));
    assert!(!rules.method_name_regex().is_match(""));
}

#[test]
fn test_validation_rules_clone() {
    let rules1 = ValidationRules::default();
    let rules2 = rules1.clone();

    // Should have same values
    assert_eq!(rules1.max_message_size, rules2.max_message_size);
    assert_eq!(rules1.max_batch_size, rules2.max_batch_size);
    assert_eq!(rules1.max_string_length, rules2.max_string_length);

    // Regex patterns should work the same
    let test_uri = "https://example.com";
    assert_eq!(
        rules1.uri_regex().is_match(test_uri),
        rules2.uri_regex().is_match(test_uri)
    );
}

// ========== Helper Functions ==========

fn create_valid_request() -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        method: "tools/list".to_string(),
        params: None,
        id: RequestId::String("test-123".to_string()),
    }
}

fn create_valid_tool() -> Tool {
    Tool {
        name: "test_tool".to_string(),
        title: Some("Test Tool".to_string()),
        description: Some("A test tool for validation".to_string()),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
            additional_properties: None,
        },
        output_schema: None,
        annotations: None,
        meta: None,
    }
}

fn create_tool_with_long_name() -> Tool {
    let mut tool = create_valid_tool();
    tool.name = "x".repeat(100);
    tool
}

fn create_valid_prompt() -> Prompt {
    Prompt {
        name: "test_prompt".to_string(),
        title: Some("Test Prompt".to_string()),
        description: Some("A test prompt".to_string()),
        arguments: Some(vec![create_prompt_argument("arg1")]),
        meta: None,
    }
}

fn create_prompt_argument(name: &str) -> PromptArgument {
    PromptArgument {
        name: name.to_string(),
        title: Some(format!("Argument {name}")),
        description: Some(format!("Description for {name}")),
        required: Some(true),
    }
}

fn create_valid_resource() -> Resource {
    Resource {
        name: "test_resource".to_string(),
        title: Some("Test Resource".to_string()),
        uri: "file://test/resource.txt".to_string(),
        description: Some("A test resource".to_string()),
        mime_type: Some("text/plain".to_string()),
        annotations: None,
        size: Some(1024),
        meta: None,
    }
}

fn create_valid_initialize_request() -> InitializeRequest {
    InitializeRequest {
        protocol_version: "2025-06-18".to_string(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "test-client".to_string(),
            title: Some("Test Client".to_string()),
            version: "1.0.0".to_string(),
        },
        _meta: None,
    }
}

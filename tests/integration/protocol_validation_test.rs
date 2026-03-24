//! Comprehensive tests for protocol validation edge cases
//! Tests deep object validation, Unicode handling, schema validation, and error accumulation

use std::collections::HashMap;
use serde_json::{json, Value};

use turbomcp_protocol::validation::*;
use turbomcp_protocol::types::*;
use turbomcp_protocol::jsonrpc::*;
use turbomcp::{McpError, McpResult};

#[tokio::test]
async fn test_deep_object_validation_maximum_depth() {
    let validator = SchemaValidator::new();
    
    // Create deeply nested object (beyond typical limits)
    let mut deep_object = json!("base");
    for _ in 0..1000 {
        deep_object = json!({ "nested": deep_object });
    }
    
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(Value::Number(1.into())),
        method: "test_method".to_string(),
        params: Some(deep_object),
    };
    
    let result = validator.validate_request(&request).await;
    assert!(result.is_err());
    
    match result.unwrap_err() {
        McpError::InvalidParams(msg) => {
            assert!(msg.contains("maximum depth"));
        }
        _ => panic!("Expected InvalidParams error for maximum depth"),
    }
}

#[tokio::test]
async fn test_large_array_validation_memory_constraints() {
    let validator = SchemaValidator::with_limits(ValidationLimits {
        max_array_length: 10000,
        max_string_length: 1000000,
        max_object_depth: 100,
        max_object_properties: 1000,
    });
    
    // Create array that exceeds memory-safe limits
    let large_array: Vec<Value> = (0..20000)
        .map(|i| json!({ "id": i, "data": "x".repeat(1000) }))
        .collect();
    
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(Value::Number(1.into())),
        method: "test_method".to_string(),
        params: Some(Value::Array(large_array)),
    };
    
    let result = validator.validate_request(&request).await;
    assert!(result.is_err());
    
    match result.unwrap_err() {
        McpError::InvalidParams(msg) => {
            assert!(msg.contains("array length") || msg.contains("memory limit"));
        }
        _ => panic!("Expected InvalidParams error for large array"),
    }
}

#[tokio::test]
async fn test_unicode_string_validation_edge_cases() {
    let validator = SchemaValidator::new();
    
    // Test various Unicode edge cases
    let unicode_test_cases = vec![
        // Valid Unicode
        ("valid_unicode", "Hello 世界 🌍", true),
        // Emoji sequences
        ("emoji_sequence", "👨‍👩‍👧‍👦", true),
        // Zero-width characters
        ("zero_width", "test\u{200B}string", true),
        // Surrogate pairs (should be handled properly)
        ("high_unicode", "\u{1F600}", true), // 😀
        // Very long Unicode string
        ("long_unicode", "🌟".repeat(10000), false), // Should exceed limits
        // Mixed scripts
        ("mixed_scripts", "Hello مرحبا こんにちは", true),
        // Right-to-left text
        ("rtl_text", "مرحبا بالعالم", true),
        // Combining characters
        ("combining", "é́", true), // e + combining acute + combining acute
        // Control characters (should be rejected)
        ("control_chars", "test\x00string", false),
        // Invalid UTF-8 sequences (should be caught by serde_json, but test anyway)
        ("replacement_char", "test\u{FFFD}replacement", true),
    ];
    
    for (test_name, text, should_pass) in unicode_test_cases {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::Number(1.into())),
            method: "test_method".to_string(),
            params: Some(json!({ "text": text })),
        };
        
        let result = validator.validate_request(&request).await;
        
        if should_pass {
            assert!(result.is_ok(), "Test case '{}' should pass but failed: {:?}", test_name, result);
        } else {
            assert!(result.is_err(), "Test case '{}' should fail but passed", test_name);
        }
    }
}

#[tokio::test]
async fn test_schema_validation_complex_types() {
    let validator = SchemaValidator::new();
    
    // Test complex nested schemas
    let complex_schema = json!({
        "type": "object",
        "properties": {
            "users": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer", "minimum": 1 },
                        "name": { "type": "string", "minLength": 1, "maxLength": 100 },
                        "email": { 
                            "type": "string", 
                            "pattern": "^[^@]+@[^@]+\\.[^@]+$" 
                        },
                        "preferences": {
                            "type": "object",
                            "properties": {
                                "theme": { "enum": ["light", "dark", "auto"] },
                                "notifications": {
                                    "type": "object",
                                    "properties": {
                                        "email": { "type": "boolean" },
                                        "push": { "type": "boolean" },
                                        "frequency": { "enum": ["immediate", "hourly", "daily"] }
                                    },
                                    "required": ["email", "push"]
                                }
                            },
                            "required": ["theme"]
                        }
                    },
                    "required": ["id", "name", "email"]
                }
            },
            "metadata": {
                "type": "object",
                "additionalProperties": true
            }
        },
        "required": ["users"]
    });
    
    // Valid complex data
    let valid_data = json!({
        "users": [
            {
                "id": 1,
                "name": "John Doe",
                "email": "john@example.com",
                "preferences": {
                    "theme": "dark",
                    "notifications": {
                        "email": true,
                        "push": false,
                        "frequency": "daily"
                    }
                }
            },
            {
                "id": 2,
                "name": "Jane Smith",
                "email": "jane@test.org",
                "preferences": {
                    "theme": "light",
                    "notifications": {
                        "email": false,
                        "push": true,
                        "frequency": "immediate"
                    }
                }
            }
        ],
        "metadata": {
            "version": "1.0",
            "created_at": "2024-01-01T00:00:00Z",
            "custom_field": 42
        }
    });
    
    let result = validator.validate_against_schema(&valid_data, &complex_schema).await;
    assert!(result.is_ok());
    
    // Invalid data - missing required field
    let invalid_data = json!({
        "users": [
            {
                "id": 1,
                "name": "John Doe",
                // Missing email
                "preferences": {
                    "theme": "dark",
                    "notifications": {
                        "email": true,
                        "push": false
                        // Missing frequency
                    }
                }
            }
        ]
    });
    
    let result = validator.validate_against_schema(&invalid_data, &complex_schema).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_validation_error_accumulation() {
    let validator = SchemaValidator::new();
    
    // Schema with multiple validation rules
    let schema = json!({
        "type": "object",
        "properties": {
            "id": { "type": "integer", "minimum": 1, "maximum": 1000 },
            "name": { "type": "string", "minLength": 2, "maxLength": 50 },
            "email": { "type": "string", "pattern": "^[^@]+@[^@]+\\.[^@]+$" },
            "age": { "type": "integer", "minimum": 0, "maximum": 150 },
            "tags": { 
                "type": "array", 
                "maxItems": 10,
                "items": { "type": "string", "maxLength": 20 }
            }
        },
        "required": ["id", "name", "email"]
    });
    
    // Data with multiple validation errors
    let invalid_data = json!({
        "id": -5,                    // Below minimum
        "name": "A",                 // Too short
        "email": "invalid-email",    // Invalid format
        "age": 200,                  // Above maximum
        "tags": [
            "valid_tag",
            "this_tag_is_way_too_long_for_the_schema", // Exceeds maxLength
            "another_valid_tag"
        ],
        "extra_field": "not_allowed" // Additional property (if strict)
    });
    
    let result = validator.validate_against_schema(&invalid_data, &schema).await;
    assert!(result.is_err());
    
    // Check that all errors are reported
    match result.unwrap_err() {
        McpError::InvalidParams(msg) => {
            assert!(msg.contains("id"));
            assert!(msg.contains("name"));
            assert!(msg.contains("email"));
            assert!(msg.contains("age"));
            assert!(msg.contains("tags"));
        }
        _ => panic!("Expected InvalidParams with multiple errors"),
    }
}

#[tokio::test]
async fn test_validation_warning_vs_error_classification() {
    let validator = SchemaValidator::with_strict_mode(false);
    
    let schema = json!({
        "type": "object",
        "properties": {
            "required_field": { "type": "string" },
            "optional_field": { "type": "string" }
        },
        "required": ["required_field"],
        "additionalProperties": false
    });
    
    let data_with_warnings = json!({
        "required_field": "present",
        "unknown_field": "should_warn_not_error",  // Warning in non-strict mode
        "optional_field": "also_present"
    });
    
    let result = validator.validate_against_schema(&data_with_warnings, &schema).await;
    
    // Should succeed but with warnings
    assert!(result.is_ok());
    
    let validation_result = result.unwrap();
    assert!(validation_result.has_warnings());
    assert!(!validation_result.has_errors());
    
    let warnings = validation_result.get_warnings();
    assert!(!warnings.is_empty());
    assert!(warnings.iter().any(|w| w.contains("unknown_field")));
}

#[tokio::test]
async fn test_validation_context_path_tracking() {
    let validator = SchemaValidator::new();
    
    let schema = json!({
        "type": "object",
        "properties": {
            "users": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "profile": {
                            "type": "object",
                            "properties": {
                                "age": { "type": "integer", "minimum": 0 }
                            }
                        }
                    }
                }
            }
        }
    });
    
    let data_with_nested_error = json!({
        "users": [
            {
                "profile": {
                    "age": "not_a_number"  // Error at users[0].profile.age
                }
            },
            {
                "profile": {
                    "age": -5  // Error at users[1].profile.age
                }
            }
        ]
    });
    
    let result = validator.validate_against_schema(&data_with_nested_error, &schema).await;
    assert!(result.is_err());
    
    match result.unwrap_err() {
        McpError::InvalidParams(msg) => {
            // Should include path information
            assert!(msg.contains("users[0].profile.age") || msg.contains("users.0.profile.age"));
            assert!(msg.contains("users[1].profile.age") || msg.contains("users.1.profile.age"));
        }
        _ => panic!("Expected InvalidParams with path information"),
    }
}

#[tokio::test]
async fn test_custom_validation_rules() {
    let mut validator = SchemaValidator::new();
    
    // Add custom validation rule
    validator.add_custom_rule("business_email", |value| {
        if let Some(email) = value.as_str() {
            // Business emails must not be from common free providers
            let free_domains = ["gmail.com", "yahoo.com", "hotmail.com"];
            let domain = email.split('@').nth(1).unwrap_or("");
            !free_domains.contains(&domain)
        } else {
            false
        }
    });
    
    let schema = json!({
        "type": "object",
        "properties": {
            "work_email": { "custom": "business_email" }
        }
    });
    
    // Test valid business email
    let valid_data = json!({
        "work_email": "user@company.com"
    });
    
    let result = validator.validate_against_schema(&valid_data, &schema).await;
    assert!(result.is_ok());
    
    // Test invalid business email
    let invalid_data = json!({
        "work_email": "user@gmail.com"
    });
    
    let result = validator.validate_against_schema(&invalid_data, &schema).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_tool_parameter_validation_edge_cases() {
    let validator = ToolValidator::new();
    
    // Tool with complex parameter schema
    let tool_schema = ToolDefinition {
        name: "file_processor".to_string(),
        description: "Process files with various options".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "files": {
                    "type": "array",
                    "items": { "type": "string", "pattern": "^[^/]+\\.[a-z]+$" },
                    "minItems": 1,
                    "maxItems": 100
                },
                "options": {
                    "type": "object",
                    "properties": {
                        "format": { "enum": ["json", "xml", "csv"] },
                        "compression": { "type": "boolean" },
                        "encoding": { "type": "string", "default": "utf-8" },
                        "filters": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "field": { "type": "string" },
                                    "operator": { "enum": ["eq", "ne", "gt", "lt", "contains"] },
                                    "value": { "oneOf": [
                                        { "type": "string" },
                                        { "type": "number" },
                                        { "type": "boolean" }
                                    ]}
                                },
                                "required": ["field", "operator", "value"]
                            }
                        }
                    },
                    "required": ["format"]
                }
            },
            "required": ["files", "options"]
        }),
    };
    
    // Valid tool call
    let valid_call = ToolCall {
        name: "file_processor".to_string(),
        arguments: json!({
            "files": ["data.csv", "config.json"],
            "options": {
                "format": "json",
                "compression": true,
                "encoding": "utf-8",
                "filters": [
                    {
                        "field": "status",
                        "operator": "eq",
                        "value": "active"
                    },
                    {
                        "field": "score",
                        "operator": "gt",
                        "value": 85
                    }
                ]
            }
        }),
    };
    
    let result = validator.validate_tool_call(&valid_call, &tool_schema).await;
    assert!(result.is_ok());
    
    // Invalid tool call - multiple errors
    let invalid_call = ToolCall {
        name: "file_processor".to_string(),
        arguments: json!({
            "files": [], // Empty array (violates minItems)
            "options": {
                "format": "pdf", // Invalid enum value
                "compression": "yes", // Wrong type
                "filters": [
                    {
                        "field": "status",
                        "operator": "invalid_op", // Invalid operator
                        "value": "active"
                    }
                ]
            }
        }),
    };
    
    let result = validator.validate_tool_call(&invalid_call, &tool_schema).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_initialize_request_validation() {
    let validator = InitializeValidator::new();
    
    // Valid initialize request
    let valid_request = InitializeRequest {
        protocol_version: "2025-11-25".into(),
        capabilities: ClientCapabilities {
            roots: Some(RootsCapability {
                list_changed: Some(true),
            }),
            sampling: Some(SamplingCapability {}),
            experimental: None,
        },
        client_info: ClientInfo {
            name: "test-client".to_string(),
            version: "1.0.0".to_string(),
        },
    };
    
    let result = validator.validate_initialize(&valid_request).await;
    assert!(result.is_ok());
    
    // Invalid protocol version
    let invalid_request = InitializeRequest {
        protocol_version: "invalid-version".to_string(),
        capabilities: ClientCapabilities {
            roots: None,
            sampling: None,
            experimental: None,
        },
        client_info: ClientInfo {
            name: "".to_string(), // Empty name
            version: "".to_string(), // Empty version
        },
    };
    
    let result = validator.validate_initialize(&invalid_request).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resource_validation_edge_cases() {
    let validator = ResourceValidator::new();
    
    // Test various URI edge cases
    let uri_test_cases = vec![
        // Valid URIs
        ("file:///absolute/path/file.txt", true),
        ("https://example.com/resource", true),
        ("custom-scheme://host/path", true),
        
        // Edge cases
        ("file://", false), // Missing path
        ("", false), // Empty URI
        ("not-a-uri", false), // Invalid format
        ("file:///path/with spaces", false), // Unencoded spaces
        ("https://", false), // Missing host
        ("ftp://user:pass@host/path", true), // With credentials
        
        // Very long URI
        (&format!("file:///{}", "a".repeat(10000)), false),
        
        // Unicode in URI (should be encoded)
        ("file:///café/file.txt", false), // Unencoded Unicode
        ("file:///caf%C3%A9/file.txt", true), // Properly encoded
    ];
    
    for (uri, should_be_valid) in uri_test_cases {
        let resource = Resource {
            uri: uri.to_string(),
            name: Some("test".to_string()),
            description: None,
            mime_type: Some("text/plain".to_string()),
        };
        
        let result = validator.validate_resource(&resource).await;
        
        if should_be_valid {
            assert!(result.is_ok(), "URI '{}' should be valid but failed", uri);
        } else {
            assert!(result.is_err(), "URI '{}' should be invalid but passed", uri);
        }
    }
}

#[tokio::test]
async fn test_concurrent_validation() {
    let validator = Arc::new(SchemaValidator::new());
    let schema = json!({
        "type": "object",
        "properties": {
            "id": { "type": "integer" },
            "data": { "type": "string" }
        }
    });
    
    let mut handles = vec![];
    
    // Validate many objects concurrently
    for i in 0..100 {
        let validator_clone = Arc::clone(&validator);
        let schema_clone = schema.clone();
        
        let handle = tokio::spawn(async move {
            let data = json!({
                "id": i,
                "data": format!("test_data_{}", i)
            });
            
            validator_clone.validate_against_schema(&data, &schema_clone).await
        });
        
        handles.push(handle);
    }
    
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    // All validations should succeed
    for (i, result) in results.into_iter().enumerate() {
        assert!(result.is_ok(), "Concurrent validation {} failed", i);
        assert!(result.unwrap().is_ok(), "Validation {} should succeed", i);
    }
}

// Helper types and implementations

struct ValidationLimits {
    max_array_length: usize,
    max_string_length: usize,
    max_object_depth: usize,
    max_object_properties: usize,
}

struct SchemaValidator {
    limits: ValidationLimits,
    strict_mode: bool,
    custom_rules: HashMap<String, Box<dyn Fn(&Value) -> bool + Send + Sync>>,
}

impl SchemaValidator {
    fn new() -> Self {
        Self {
            limits: ValidationLimits {
                max_array_length: 1000,
                max_string_length: 10000,
                max_object_depth: 50,
                max_object_properties: 100,
            },
            strict_mode: true,
            custom_rules: HashMap::new(),
        }
    }
    
    fn with_limits(limits: ValidationLimits) -> Self {
        Self {
            limits,
            strict_mode: true,
            custom_rules: HashMap::new(),
        }
    }
    
    fn with_strict_mode(strict: bool) -> Self {
        Self {
            limits: ValidationLimits {
                max_array_length: 1000,
                max_string_length: 10000,
                max_object_depth: 50,
                max_object_properties: 100,
            },
            strict_mode: strict,
            custom_rules: HashMap::new(),
        }
    }
    
    fn add_custom_rule<F>(&mut self, name: &str, rule: F)
    where
        F: Fn(&Value) -> bool + Send + Sync + 'static,
    {
        self.custom_rules.insert(name.to_string(), Box::new(rule));
    }
    
    async fn validate_request(&self, request: &JsonRpcRequest) -> McpResult<ValidationResult> {
        // Implementation would go here
        Ok(ValidationResult::success())
    }
    
    async fn validate_against_schema(&self, data: &Value, schema: &Value) -> McpResult<ValidationResult> {
        // Implementation would go here
        Ok(ValidationResult::success())
    }
}

struct ValidationResult {
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl ValidationResult {
    fn success() -> Self {
        Self {
            errors: vec![],
            warnings: vec![],
        }
    }
    
    fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
    
    fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
    
    fn get_warnings(&self) -> &[String] {
        &self.warnings
    }
}

struct ToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

struct ToolCall {
    name: String,
    arguments: Value,
}

struct ToolValidator;

impl ToolValidator {
    fn new() -> Self {
        Self
    }
    
    async fn validate_tool_call(&self, call: &ToolCall, definition: &ToolDefinition) -> McpResult<()> {
        // Implementation would go here
        Ok(())
    }
}

struct InitializeRequest {
    protocol_version: String,
    capabilities: ClientCapabilities,
    client_info: ClientInfo,
}

struct ClientCapabilities {
    roots: Option<RootsCapability>,
    sampling: Option<SamplingCapability>,
    experimental: Option<Value>,
}

struct RootsCapability {
    list_changed: Option<bool>,
}

struct SamplingCapability;

struct ClientInfo {
    name: String,
    version: String,
}

struct InitializeValidator;

impl InitializeValidator {
    fn new() -> Self {
        Self
    }
    
    async fn validate_initialize(&self, request: &InitializeRequest) -> McpResult<()> {
        // Implementation would go here
        Ok(())
    }
}

struct Resource {
    uri: String,
    name: Option<String>,
    description: Option<String>,
    mime_type: Option<String>,
}

struct ResourceValidator;

impl ResourceValidator {
    fn new() -> Self {
        Self
    }
    
    async fn validate_resource(&self, resource: &Resource) -> McpResult<()> {
        // Implementation would go here
        Ok(())
    }
}

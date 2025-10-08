//! MCP 2025-06-18 Compliance Validation Tests
//!
//! This test suite validates TurboMCP's compliance with the official MCP specification,
//! specifically addressing the gaps identified through comprehensive dogfooding and
//! validated against the authoritative JSON Schema (schema/2025-06-18/schema.json).

use serde_json::json;
use turbomcp_protocol::types::*;
use turbomcp_protocol::validation::ProtocolValidator;

/// Test Gap #1: StopReason enum serialization uses camelCase per spec example (sampling.mdx:102)
#[test]
fn test_stop_reason_camel_case_serialization() {
    use StopReason::*;

    // Test all variants serialize to camelCase (not snake_case)
    assert_eq!(serde_json::to_string(&EndTurn).unwrap(), "\"endTurn\"");
    assert_eq!(serde_json::to_string(&MaxTokens).unwrap(), "\"maxTokens\"");
    assert_eq!(
        serde_json::to_string(&StopSequence).unwrap(),
        "\"stopSequence\""
    );
    assert_eq!(
        serde_json::to_string(&ContentFilter).unwrap(),
        "\"contentFilter\""
    );
    assert_eq!(serde_json::to_string(&ToolUse).unwrap(), "\"toolUse\"");
}

#[test]
fn test_stop_reason_camel_case_deserialization() {
    use StopReason::*;

    // Test deserialization from camelCase
    assert_eq!(
        serde_json::from_str::<StopReason>("\"endTurn\"").unwrap(),
        EndTurn
    );
    assert_eq!(
        serde_json::from_str::<StopReason>("\"maxTokens\"").unwrap(),
        MaxTokens
    );
    assert_eq!(
        serde_json::from_str::<StopReason>("\"stopSequence\"").unwrap(),
        StopSequence
    );
    assert_eq!(
        serde_json::from_str::<StopReason>("\"contentFilter\"").unwrap(),
        ContentFilter
    );
    assert_eq!(
        serde_json::from_str::<StopReason>("\"toolUse\"").unwrap(),
        ToolUse
    );
}

#[test]
fn test_create_message_result_with_stop_reason() {
    // Test full CreateMessageResult with StopReason enum
    let result = CreateMessageResult {
        role: Role::Assistant,
        content: Content::Text(TextContent {
            text: "The capital of France is Paris.".to_string(),
            annotations: None,
            meta: None,
        }),
        model: "claude-3-sonnet-20240307".to_string(),
        stop_reason: Some(StopReason::EndTurn),
        _meta: None,
    };

    // Serialize
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["stopReason"], "endTurn"); // ← Must be camelCase!
    assert_eq!(json["role"], "assistant");
    assert_eq!(json["model"], "claude-3-sonnet-20240307");

    // Round-trip
    let json_str = serde_json::to_string(&result).unwrap();
    let deserialized: CreateMessageResult = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(deserialized.model, "claude-3-sonnet-20240307");
}

#[test]
fn test_interop_with_spec_example() {
    // Test against exact JSON from MCP spec (sampling.mdx:91-104)
    let spec_json = json!({
        "role": "assistant",
        "content": {
            "type": "text",
            "text": "The capital of France is Paris."
        },
        "model": "claude-3-sonnet-20240307",
        "stopReason": "endTurn"
    });

    let result: CreateMessageResult = serde_json::from_value(spec_json).unwrap();
    assert_eq!(result.role, Role::Assistant);
    assert_eq!(result.model, "claude-3-sonnet-20240307");
    assert_eq!(result.stop_reason, Some(StopReason::EndTurn));
}

/// Test Gap #6: Priority range validation (0.0-1.0) per schema.json:1346-1370
#[test]
fn test_priority_range_validation_valid() {
    let validator = ProtocolValidator::new();

    // Valid priorities (0.0-1.0)
    let valid = ModelPreferences {
        cost_priority: Some(0.0),
        speed_priority: Some(0.5),
        intelligence_priority: Some(1.0),
        hints: None,
    };

    let result = validator.validate_model_preferences(&valid);
    assert!(result.is_valid(), "Valid priorities should pass validation");
    assert!(result.errors().is_empty());
}

#[test]
fn test_priority_range_validation_out_of_range() {
    let validator = ProtocolValidator::new();

    // Invalid: below range
    let invalid_low = ModelPreferences {
        cost_priority: Some(-0.1),
        speed_priority: None,
        intelligence_priority: None,
        hints: None,
    };

    let result = validator.validate_model_preferences(&invalid_low);
    assert!(!result.is_valid(), "Priority < 0.0 should fail validation");
    assert_eq!(result.errors().len(), 1);
    assert_eq!(result.errors()[0].code, "PRIORITY_OUT_OF_RANGE");

    // Invalid: above range
    let invalid_high = ModelPreferences {
        cost_priority: None,
        speed_priority: None,
        intelligence_priority: Some(1.5),
        hints: None,
    };

    let result = validator.validate_model_preferences(&invalid_high);
    assert!(!result.is_valid(), "Priority > 1.0 should fail validation");
    assert_eq!(result.errors().len(), 1);
    assert_eq!(result.errors()[0].code, "PRIORITY_OUT_OF_RANGE");
}

#[test]
fn test_priority_range_validation_boundary() {
    let validator = ProtocolValidator::new();

    // Boundary values should be valid
    let boundary = ModelPreferences {
        cost_priority: Some(0.0),
        speed_priority: Some(1.0),
        intelligence_priority: Some(0.5),
        hints: None,
    };

    let result = validator.validate_model_preferences(&boundary);
    assert!(
        result.is_valid(),
        "Boundary values 0.0 and 1.0 should be valid"
    );
}

#[test]
fn test_priority_range_validation_multiple_errors() {
    let validator = ProtocolValidator::new();

    // Multiple out-of-range priorities
    let invalid = ModelPreferences {
        cost_priority: Some(-0.5),
        speed_priority: Some(2.0),
        intelligence_priority: Some(1.5),
        hints: None,
    };

    let result = validator.validate_model_preferences(&invalid);
    assert!(!result.is_valid());
    assert_eq!(
        result.errors().len(),
        3,
        "All three invalid priorities should be reported"
    );
}

/// Test Gap #9: ElicitResult content validation per schema.json:634
#[test]
fn test_elicit_result_accept_requires_content() {
    let validator = ProtocolValidator::new();

    // Accept without content = ERROR
    let invalid = ElicitResult {
        action: ElicitationAction::Accept,
        content: None,
        _meta: None,
    };

    let result = validator.validate_elicit_result(&invalid);
    assert!(!result.is_valid(), "Accept without content should fail");
    assert_eq!(result.errors().len(), 1);
    assert_eq!(result.errors()[0].code, "MISSING_CONTENT_ON_ACCEPT");
}

#[test]
fn test_elicit_result_accept_with_content_valid() {
    let validator = ProtocolValidator::new();

    // Accept with content = VALID
    let mut content = std::collections::HashMap::new();
    content.insert("email".to_string(), json!("user@example.com"));
    content.insert("age".to_string(), json!(30));

    let valid = ElicitResult {
        action: ElicitationAction::Accept,
        content: Some(content),
        _meta: None,
    };

    let result = validator.validate_elicit_result(&valid);
    assert!(result.is_valid(), "Accept with content should be valid");
}

#[test]
fn test_elicit_result_decline_with_content_warning() {
    let validator = ProtocolValidator::new();

    // Decline with content = WARNING (not recommended but not error)
    let mut content = std::collections::HashMap::new();
    content.insert("foo".to_string(), json!("bar"));

    let warning = ElicitResult {
        action: ElicitationAction::Decline,
        content: Some(content),
        _meta: None,
    };

    let result = validator.validate_elicit_result(&warning);
    assert!(result.is_valid(), "Decline with content is valid");
    assert!(result.has_warnings(), "But should have warning");
    assert_eq!(result.warnings()[0].code, "UNEXPECTED_CONTENT");
}

#[test]
fn test_elicit_result_cancel_without_content_valid() {
    let validator = ProtocolValidator::new();

    // Cancel without content = VALID
    let valid = ElicitResult {
        action: ElicitationAction::Cancel,
        content: None,
        _meta: None,
    };

    let result = validator.validate_elicit_result(&valid);
    assert!(result.is_valid());
    assert!(!result.has_warnings());
}

/// Test Gap #5: Schema structure validation per schema.json:585
#[test]
fn test_elicitation_schema_must_be_object() {
    let validator = ProtocolValidator::new();

    // Invalid: schema type not "object"
    let invalid = ElicitationSchema {
        schema_type: "array".to_string(),
        properties: std::collections::HashMap::new(),
        required: None,
        additional_properties: None,
    };

    let result = validator.validate_elicitation_schema(&invalid);
    assert!(!result.is_valid(), "Non-object schema should fail");
    assert_eq!(result.errors().len(), 1);
    assert_eq!(result.errors()[0].code, "SCHEMA_NOT_OBJECT");
}

#[test]
fn test_elicitation_schema_object_valid() {
    let validator = ProtocolValidator::new();

    // Valid: object schema with properties
    let mut properties = std::collections::HashMap::new();
    properties.insert(
        "email".to_string(),
        PrimitiveSchemaDefinition::String {
            title: Some("Email Address".to_string()),
            description: Some("Your email".to_string()),
            format: Some("email".to_string()),
            min_length: None,
            max_length: None,
            enum_values: None,
            enum_names: None,
        },
    );

    let valid = ElicitationSchema {
        schema_type: "object".to_string(),
        properties,
        required: Some(vec!["email".to_string()]),
        additional_properties: Some(false),
    };

    let result = validator.validate_elicitation_schema(&valid);
    assert!(result.is_valid(), "Valid object schema should pass");
}

#[test]
fn test_elicitation_schema_additional_properties_warning() {
    let validator = ProtocolValidator::new();

    // additionalProperties=true should warn (not recommended for flat schemas)
    let schema = ElicitationSchema {
        schema_type: "object".to_string(),
        properties: std::collections::HashMap::new(),
        required: None,
        additional_properties: Some(true), // ← Not recommended
    };

    let result = validator.validate_elicitation_schema(&schema);
    assert!(result.is_valid(), "Still valid");
    assert!(result.has_warnings(), "But should warn");
    assert_eq!(
        result.warnings()[0].code,
        "ADDITIONAL_PROPERTIES_NOT_RECOMMENDED"
    );
}

/// Test Gap #8: Enum/enumNames length validation per schema.json:679-708
#[test]
fn test_enum_names_length_match() {
    let validator = ProtocolValidator::new();

    // Matching lengths = VALID
    let valid = PrimitiveSchemaDefinition::String {
        title: None,
        description: None,
        format: None,
        min_length: None,
        max_length: None,
        enum_values: Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
        enum_names: Some(vec![
            "Option A".to_string(),
            "Option B".to_string(),
            "Option C".to_string(),
        ]),
    };

    let mut properties = std::collections::HashMap::new();
    properties.insert("choice".to_string(), valid);

    let schema = ElicitationSchema {
        schema_type: "object".to_string(),
        properties,
        required: None,
        additional_properties: None,
    };

    let result = validator.validate_elicitation_schema(&schema);
    assert!(
        result.is_valid(),
        "Matching enum/enumNames lengths should be valid"
    );
}

#[test]
fn test_enum_names_length_mismatch() {
    let validator = ProtocolValidator::new();

    // Mismatched lengths = ERROR
    let invalid = PrimitiveSchemaDefinition::String {
        title: None,
        description: None,
        format: None,
        min_length: None,
        max_length: None,
        enum_values: Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
        enum_names: Some(vec!["Option A".to_string()]), // Only 1!
    };

    let mut properties = std::collections::HashMap::new();
    properties.insert("choice".to_string(), invalid);

    let schema = ElicitationSchema {
        schema_type: "object".to_string(),
        properties,
        required: None,
        additional_properties: None,
    };

    let result = validator.validate_elicitation_schema(&schema);
    assert!(
        !result.is_valid(),
        "Mismatched enum/enumNames lengths should fail"
    );
    assert_eq!(result.errors().len(), 1);
    assert_eq!(result.errors()[0].code, "ENUM_NAMES_LENGTH_MISMATCH");
}

/// Test Gap #4: Format validation per schema.json:2244-2251
#[test]
fn test_string_format_validation_email() {
    // Valid emails
    assert!(ProtocolValidator::validate_string_format("user@example.com", "email").is_ok());
    assert!(
        ProtocolValidator::validate_string_format("test.user@sub.example.co.uk", "email").is_ok()
    );

    // Invalid emails
    assert!(ProtocolValidator::validate_string_format("not-an-email", "email").is_err());
    assert!(ProtocolValidator::validate_string_format("@example.com", "email").is_err());
    assert!(ProtocolValidator::validate_string_format("user@", "email").is_err());
    assert!(ProtocolValidator::validate_string_format("user", "email").is_err());
}

#[test]
fn test_string_format_validation_uri() {
    // Valid URIs
    assert!(ProtocolValidator::validate_string_format("https://example.com", "uri").is_ok());
    assert!(ProtocolValidator::validate_string_format("http://localhost:8080", "uri").is_ok());
    assert!(ProtocolValidator::validate_string_format("/path/to/resource", "uri").is_ok());

    // Invalid URIs
    assert!(ProtocolValidator::validate_string_format("not a uri", "uri").is_err());
}

#[test]
fn test_string_format_validation_date() {
    // Valid ISO 8601 dates
    assert!(ProtocolValidator::validate_string_format("2025-10-07", "date").is_ok());
    assert!(ProtocolValidator::validate_string_format("2025-01-01", "date").is_ok());

    // Invalid dates
    assert!(ProtocolValidator::validate_string_format("10/07/2025", "date").is_err());
    assert!(ProtocolValidator::validate_string_format("2025-1-7", "date").is_err());
    assert!(ProtocolValidator::validate_string_format("2025-13-01", "date").is_ok()); // Doesn't validate month/day ranges
    assert!(ProtocolValidator::validate_string_format("not-a-date", "date").is_err());
}

#[test]
fn test_string_format_validation_datetime() {
    // Valid ISO 8601 datetimes
    assert!(ProtocolValidator::validate_string_format("2025-10-07T15:30:00Z", "date-time").is_ok());
    assert!(
        ProtocolValidator::validate_string_format("2025-10-07T15:30:00.123Z", "date-time").is_ok()
    );

    // Invalid datetimes
    assert!(ProtocolValidator::validate_string_format("2025-10-07", "date-time").is_err());
    assert!(ProtocolValidator::validate_string_format("2025-10-07 15:30:00", "date-time").is_err());
    assert!(ProtocolValidator::validate_string_format("not-a-datetime", "date-time").is_err());
}

#[test]
fn test_string_format_validation_unknown() {
    // Unknown formats don't fail (forward compatibility)
    assert!(ProtocolValidator::validate_string_format("anything", "custom-format").is_ok());
}

#[test]
fn test_unknown_format_warning() {
    let validator = ProtocolValidator::new();

    // Unknown format should warn
    let schema_with_unknown = PrimitiveSchemaDefinition::String {
        title: None,
        description: None,
        format: Some("unknown-format".to_string()),
        min_length: None,
        max_length: None,
        enum_values: None,
        enum_names: None,
    };

    let mut properties = std::collections::HashMap::new();
    properties.insert("field".to_string(), schema_with_unknown);

    let schema = ElicitationSchema {
        schema_type: "object".to_string(),
        properties,
        required: None,
        additional_properties: None,
    };

    let result = validator.validate_elicitation_schema(&schema);
    assert!(result.is_valid(), "Unknown format is valid");
    assert!(result.has_warnings(), "But should warn");
    assert_eq!(result.warnings()[0].code, "UNKNOWN_STRING_FORMAT");
}

/// Comprehensive integration test
#[test]
fn test_full_mcp_compliance_scenario() {
    let validator = ProtocolValidator::new();

    // Create a full elicitation schema
    let mut properties = std::collections::HashMap::new();

    // Email field with format validation
    properties.insert(
        "email".to_string(),
        PrimitiveSchemaDefinition::String {
            title: Some("Email Address".to_string()),
            description: Some("Your email address".to_string()),
            format: Some("email".to_string()),
            min_length: Some(5),
            max_length: Some(100),
            enum_values: None,
            enum_names: None,
        },
    );

    // Choice field with enum
    properties.insert(
        "priority".to_string(),
        PrimitiveSchemaDefinition::String {
            title: Some("Priority Level".to_string()),
            description: None,
            format: None,
            min_length: None,
            max_length: None,
            enum_values: Some(vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
            ]),
            enum_names: Some(vec![
                "Low".to_string(),
                "Medium".to_string(),
                "High".to_string(),
            ]),
        },
    );

    let schema = ElicitationSchema {
        schema_type: "object".to_string(),
        properties,
        required: Some(vec!["email".to_string()]),
        additional_properties: Some(false),
    };

    // Validate schema
    let result = validator.validate_elicitation_schema(&schema);
    assert!(result.is_valid(), "Comprehensive schema should be valid");

    // Test model preferences
    let prefs = ModelPreferences {
        cost_priority: Some(0.3),
        speed_priority: Some(0.5),
        intelligence_priority: Some(0.8),
        hints: None,
    };

    let prefs_result = validator.validate_model_preferences(&prefs);
    assert!(prefs_result.is_valid(), "Valid priorities should pass");

    // Test elicit result
    let mut content = std::collections::HashMap::new();
    content.insert("email".to_string(), json!("test@example.com"));
    content.insert("priority".to_string(), json!("high"));

    let elicit_result = ElicitResult {
        action: ElicitationAction::Accept,
        content: Some(content),
        _meta: None,
    };

    let elicit_validation = validator.validate_elicit_result(&elicit_result);
    assert!(
        elicit_validation.is_valid(),
        "Complete elicit result should be valid"
    );

    // Test CreateMessageResult with StopReason
    let message_result = CreateMessageResult {
        role: Role::Assistant,
        content: Content::Text(TextContent {
            text: "Response text".to_string(),
            annotations: None,
            meta: None,
        }),
        model: "claude-3-sonnet".to_string(),
        stop_reason: Some(StopReason::EndTurn),
        _meta: None,
    };

    // Verify serialization
    let json = serde_json::to_value(&message_result).unwrap();
    assert_eq!(json["stopReason"], "endTurn");
}

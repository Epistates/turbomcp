//! # MCP Schema Compliance Tests
//!
//! This module contains comprehensive tests to ensure 100% compliance with the
//! Model Context Protocol specification. These tests validate every message type,
//! schema constraint, and protocol requirement against the official MCP spec.
//!
//! ## Test Strategy
//!
//! 1. **Schema Validation**: Every message type must match MCP schema exactly
//! 2. **Constraint Testing**: All validation rules must be enforced per spec
//! 3. **Edge Case Testing**: Boundary conditions and error scenarios
//! 4. **Property Testing**: Generative testing for comprehensive coverage
//!
//! ## Reference
//! - MCP Specification: draft version (latest)
//! - Test against: `/reference/modelcontextprotocol/docs/specification/draft/schema.mdx`

use proptest::prelude::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use turbomcp_protocol::{
    jsonrpc::*,
    types::*,
    validation::*,
    *,
};

// =============================================================================
// Test Utilities
// =============================================================================

/// Creates a valid MCP request ID
fn valid_request_id() -> RequestId {
    RequestId::from("test-request-123")
}

/// Creates a valid URI per MCP specification
fn valid_uri() -> String {
    "file:///test/resource.txt".to_string()
}

/// Creates a valid MIME type
fn valid_mime_type() -> String {
    "text/plain".to_string()
}

/// Creates base64 encoded test data
fn valid_base64_data() -> String {
    base64::engine::general_purpose::STANDARD.encode("test data")
}

// =============================================================================
// JSON-RPC 2.0 Compliance Tests
// =============================================================================

#[cfg(test)]
mod jsonrpc_compliance {
    use super::*;

    #[test]
    fn test_jsonrpc_request_structure() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: valid_request_id(),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            })),
        };

        // Validate JSON-RPC 2.0 structure
        assert_eq!(request.jsonrpc, JsonRpcVersion::V2_0);
        assert!(request.id.to_string().len() > 0);
        assert!(!request.method.is_empty());

        // Validate serialization matches spec
        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert!(serialized["id"].is_string() || serialized["id"].is_number());
        assert!(serialized["method"].is_string());
    }

    #[test]
    fn test_jsonrpc_response_structure() {
        let response = JsonRpcResponse {
            jsonrpc: JsonRpcVersion::V2_0,
            id: valid_request_id(),
            result: json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        };

        // Validate structure
        assert_eq!(response.jsonrpc, JsonRpcVersion::V2_0);
        assert!(response.id.to_string().len() > 0);

        // Validate serialization
        let serialized = serde_json::to_value(&response).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert!(serialized["result"].is_object());
    }

    #[test]
    fn test_jsonrpc_error_structure() {
        let error = JsonRpcError {
            jsonrpc: JsonRpcVersion::V2_0,
            id: valid_request_id(),
            error: JsonRpcErrorCode::MethodNotFound {
                message: "Method not found: invalid/method".to_string(),
                data: None,
            },
        };

        // Validate error structure per JSON-RPC 2.0
        let serialized = serde_json::to_value(&error).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert!(serialized["error"]["code"].is_number());
        assert!(serialized["error"]["message"].is_string());
    }

    #[test]
    fn test_jsonrpc_notification_structure() {
        let notification = JsonRpcNotification {
            jsonrpc: JsonRpcVersion::V2_0,
            method: "notifications/initialized".to_string(),
            params: Some(json!({})),
        };

        // Validate notification has no ID field
        let serialized = serde_json::to_value(&notification).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert!(serialized["method"].is_string());
        assert!(!serialized.as_object().unwrap().contains_key("id"));
    }

    #[test]
    fn test_jsonrpc_batch_requests() {
        let batch = JsonRpcBatch::Requests(vec![
            JsonRpcRequest {
                jsonrpc: JsonRpcVersion::V2_0,
                id: RequestId::from("1"),
                method: "tools/list".to_string(),
                params: None,
            },
            JsonRpcRequest {
                jsonrpc: JsonRpcVersion::V2_0,
                id: RequestId::from("2"),
                method: "prompts/list".to_string(),
                params: None,
            },
        ]);

        // Validate batch structure
        let serialized = serde_json::to_value(&batch).unwrap();
        assert!(serialized.is_array());
        let array = serialized.as_array().unwrap();
        assert_eq!(array.len(), 2);

        for item in array {
            assert_eq!(item["jsonrpc"], "2.0");
            assert!(item["id"].is_string() || item["id"].is_number());
        }
    }
}

// =============================================================================
// MCP Message Type Schema Compliance
// =============================================================================

#[cfg(test)]
mod message_schema_compliance {
    use super::*;

    #[test]
    fn test_initialize_request_schema() {
        let request = InitializeRequest {
            id: valid_request_id(),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "initialize".to_string(),
            params: InitializeParams {
                protocol_version: "2025-06-18".to_string(),
                capabilities: ClientCapabilities::default(),
                client_info: Implementation {
                    name: "test-client".to_string(),
                    version: "1.0.0".to_string(),
                    title: Some("Test Client".to_string()),
                    website_url: Some("https://example.com".to_string()),
                    #[cfg(feature = "mcp-draft")]
                    description: None,
                    #[cfg(feature = "mcp-icons")]
                    icons: None,
                },
                meta: None,
            },
        };

        // Validate required fields per MCP spec
        assert_eq!(request.method, "initialize");
        assert_eq!(request.params.protocol_version, "2025-06-18");
        assert!(!request.params.client_info.name.is_empty());
        assert!(!request.params.client_info.version.is_empty());

        // Validate JSON structure matches spec
        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["method"], "initialize");
        assert!(serialized["params"]["protocolVersion"].is_string());
        assert!(serialized["params"]["capabilities"].is_object());
        assert!(serialized["params"]["clientInfo"]["name"].is_string());
    }

    #[test]
    fn test_tool_schema_compliance() {
        let tool = Tool {
            name: "test_tool".to_string(),
            title: Some("Test Tool".to_string()),
            description: Some("A test tool for validation".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = HashMap::new();
                    props.insert("param1".to_string(), json!({
                        "type": "string",
                        "description": "Test parameter"
                    }));
                    props
                }),
                required: Some(vec!["param1".to_string()]),
                additional_properties: Some(false),
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };

        // Validate tool structure per MCP spec
        assert!(!tool.name.is_empty());
        assert_eq!(tool.input_schema.schema_type, "object");

        // Validate JSON structure
        let serialized = serde_json::to_value(&tool).unwrap();
        assert!(serialized["name"].is_string());
        assert!(serialized["inputSchema"]["type"].is_string());
        assert!(serialized["inputSchema"]["properties"].is_object());
    }

    #[test]
    fn test_content_types_schema_compliance() {
        // Test TextContent
        let text_content = TextContent {
            content_type: "text".to_string(),
            text: "Test content".to_string(),
            annotations: None,
            meta: None,
        };

        let serialized = serde_json::to_value(&text_content).unwrap();
        assert_eq!(serialized["type"], "text");
        assert!(serialized["text"].is_string());

        // Test ImageContent
        let image_content = ImageContent {
            content_type: "image".to_string(),
            data: valid_base64_data(),
            mime_type: "image/png".to_string(),
            annotations: None,
            meta: None,
        };

        let serialized = serde_json::to_value(&image_content).unwrap();
        assert_eq!(serialized["type"], "image");
        assert!(serialized["data"].is_string());
        assert!(serialized["mimeType"].is_string());

        // Test AudioContent
        let audio_content = AudioContent {
            content_type: "audio".to_string(),
            data: valid_base64_data(),
            mime_type: "audio/wav".to_string(),
            annotations: None,
            meta: None,
        };

        let serialized = serde_json::to_value(&audio_content).unwrap();
        assert_eq!(serialized["type"], "audio");
        assert!(serialized["data"].is_string());
        assert!(serialized["mimeType"].is_string());
    }

    #[test]
    fn test_resource_schema_compliance() {
        let resource = Resource {
            uri: valid_uri(),
            name: "test-resource".to_string(),
            title: Some("Test Resource".to_string()),
            description: Some("A test resource".to_string()),
            mime_type: Some(valid_mime_type()),
            annotations: None,
            meta: None,
        };

        // Validate resource structure
        assert!(!resource.uri.is_empty());
        assert!(!resource.name.is_empty());

        // Validate JSON structure
        let serialized = serde_json::to_value(&resource).unwrap();
        assert!(serialized["uri"].is_string());
        assert!(serialized["name"].is_string());
    }

    #[test]
    fn test_sampling_message_schema_compliance() {
        let sampling_msg = SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                content_type: "text".to_string(),
                text: "Test message".to_string(),
                annotations: None,
                meta: None,
            }),
            meta: None,
        };

        // Validate sampling message structure
        let serialized = serde_json::to_value(&sampling_msg).unwrap();
        assert!(serialized["role"].is_string());
        assert!(serialized["content"].is_object());
    }
}

// =============================================================================
// Schema Constraint Validation Tests
// =============================================================================

#[cfg(test)]
mod constraint_validation {
    use super::*;

    #[test]
    fn test_string_length_constraints() {
        let validator = ProtocolValidator::new().with_rules(ValidationRules {
            max_string_length: 10,
            ..Default::default()
        });

        // Test tool name too long
        let long_name_tool = Tool {
            name: "this_is_a_very_long_tool_name_that_exceeds_limits".to_string(),
            title: None,
            description: None,
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };

        let result = validator.validate_tool(&long_name_tool);
        assert!(result.is_invalid());

        let errors = result.errors();
        assert!(errors.iter().any(|e| e.code.contains("TOO_LONG")));
    }

    #[test]
    fn test_array_length_constraints() {
        let validator = ProtocolValidator::new().with_rules(ValidationRules {
            max_array_length: 2,
            ..Default::default()
        });

        // Test too many tools
        let tools_result = ListToolsResult {
            tools: vec![
                create_minimal_tool("tool1"),
                create_minimal_tool("tool2"),
                create_minimal_tool("tool3"), // Exceeds limit
            ],
            next_cursor: None,
            meta: None,
        };

        let result = validator.validate_list_tools_result(&tools_result);
        assert!(result.is_invalid());
    }

    #[test]
    fn test_uri_format_validation() {
        let validator = ProtocolValidator::new();

        // Test invalid URI
        let invalid_resource = Resource {
            uri: "not-a-valid-uri".to_string(),
            name: "test".to_string(),
            title: None,
            description: None,
            mime_type: None,
            annotations: None,
            meta: None,
        };

        let result = validator.validate_resource(&invalid_resource);
        assert!(result.is_invalid());

        let errors = result.errors();
        assert!(errors.iter().any(|e| e.code.contains("INVALID_URI")));
    }

    #[test]
    fn test_method_name_validation() {
        let validator = ProtocolValidator::new();

        // Test invalid method name
        let invalid_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: valid_request_id(),
            method: "invalid-method!@#".to_string(),
            params: None,
        };

        let result = validator.validate_request(&invalid_request);
        assert!(result.is_invalid());

        let errors = result.errors();
        assert!(errors.iter().any(|e| e.code.contains("INVALID_METHOD")));
    }

    #[test]
    fn test_required_field_validation() {
        let validator = ProtocolValidator::new();

        // Test missing required field - empty tool name
        let empty_name_tool = Tool {
            name: "".to_string(), // Required field but empty
            title: None,
            description: None,
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };

        let result = validator.validate_tool(&empty_name_tool);
        assert!(result.is_invalid());

        let errors = result.errors();
        assert!(errors.iter().any(|e| e.code.contains("REQUIRED_FIELD")));
    }
}

// =============================================================================
// Error Code Compliance Tests
// =============================================================================

#[cfg(test)]
mod error_code_compliance {
    use super::*;

    #[test]
    fn test_jsonrpc_standard_error_codes() {
        // Test all JSON-RPC 2.0 standard error codes
        assert_eq!(error_codes::PARSE_ERROR, -32700);
        assert_eq!(error_codes::INVALID_REQUEST, -32600);
        assert_eq!(error_codes::METHOD_NOT_FOUND, -32601);
        assert_eq!(error_codes::INVALID_PARAMS, -32602);
        assert_eq!(error_codes::INTERNAL_ERROR, -32603);
    }

    #[test]
    fn test_mcp_specific_error_codes() {
        // Test MCP-specific error codes (application-defined range)
        assert_eq!(crate::error_codes::TOOL_NOT_FOUND, -32001);
        assert_eq!(crate::error_codes::TOOL_EXECUTION_ERROR, -32002);
        assert_eq!(crate::error_codes::PROMPT_NOT_FOUND, -32003);
        assert_eq!(crate::error_codes::RESOURCE_NOT_FOUND, -32004);
        assert_eq!(crate::error_codes::RESOURCE_ACCESS_DENIED, -32005);
        assert_eq!(crate::error_codes::CAPABILITY_NOT_SUPPORTED, -32006);
        assert_eq!(crate::error_codes::PROTOCOL_VERSION_MISMATCH, -32007);
        assert_eq!(crate::error_codes::AUTHENTICATION_REQUIRED, -32008);
        assert_eq!(crate::error_codes::RATE_LIMITED, -32009);
        assert_eq!(crate::error_codes::SERVER_OVERLOADED, -32010);
    }

    #[test]
    fn test_error_message_format() {
        let error = JsonRpcError::new(
            error_codes::TOOL_NOT_FOUND,
            "Tool not found: nonexistent_tool".to_string()
        );

        // Validate error message is concise single sentence per spec
        assert!(error.message.len() < 100); // Reasonable length limit
        assert!(!error.message.contains('\n')); // Single sentence
        assert!(error.message.starts_with("Tool not found:")); // Descriptive
    }
}

// =============================================================================
// Protocol Method Compliance Tests
// =============================================================================

#[cfg(test)]
mod method_compliance {
    use super::*;

    #[test]
    fn test_method_name_constants() {
        // Validate all method names match MCP specification exactly
        assert_eq!(methods::INITIALIZE, "initialize");
        assert_eq!(methods::INITIALIZED, "notifications/initialized");
        assert_eq!(methods::LIST_TOOLS, "tools/list");
        assert_eq!(methods::CALL_TOOL, "tools/call");
        assert_eq!(methods::LIST_PROMPTS, "prompts/list");
        assert_eq!(methods::GET_PROMPT, "prompts/get");
        assert_eq!(methods::LIST_RESOURCES, "resources/list");
        assert_eq!(methods::READ_RESOURCE, "resources/read");
        assert_eq!(methods::SUBSCRIBE, "resources/subscribe");
        assert_eq!(methods::UNSUBSCRIBE, "resources/unsubscribe");
        assert_eq!(methods::RESOURCE_UPDATED, "notifications/resources/updated");
        assert_eq!(methods::RESOURCE_LIST_CHANGED, "notifications/resources/list_changed");
        assert_eq!(methods::SET_LEVEL, "logging/setLevel");
        assert_eq!(methods::LOG_MESSAGE, "notifications/message");
        assert_eq!(methods::PROGRESS, "notifications/progress");
        assert_eq!(methods::CREATE_MESSAGE, "sampling/createMessage");
        assert_eq!(methods::LIST_ROOTS, "roots/list");
        assert_eq!(methods::ROOTS_LIST_CHANGED, "notifications/roots/list_changed");
    }

    #[test]
    fn test_request_response_pairing() {
        // Ensure every request method has corresponding response validation
        let validator = ProtocolValidator::new();

        // Test initialize request/response pair
        let init_request = create_initialize_request();
        let init_response = create_initialize_response();

        assert!(validator.validate_request(&init_request).is_valid());
        assert!(validator.validate_response(&init_response).is_valid());

        // Test tools/list request/response pair
        let list_tools_request = create_list_tools_request();
        let list_tools_response = create_list_tools_response();

        assert!(validator.validate_request(&list_tools_request).is_valid());
        assert!(validator.validate_response(&list_tools_response).is_valid());
    }
}

// =============================================================================
// Property-Based Testing
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;

    proptest! {
        #[test]
        fn test_any_valid_tool_serializes_correctly(
            name in "[a-zA-Z_][a-zA-Z0-9_]{0,50}",
            description in ".*{0,200}"
        ) {
            let tool = Tool {
                name,
                title: None,
                description: if description.is_empty() { None } else { Some(description) },
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: None,
                    required: None,
                    additional_properties: None,
                },
                output_schema: None,
                annotations: None,
                meta: None,
            };

            let validator = ProtocolValidator::new();
            let result = validator.validate_tool(&tool);

            // Property: Any tool with valid name should serialize/deserialize correctly
            if result.is_valid() {
                let serialized = serde_json::to_value(&tool).unwrap();
                let deserialized: Tool = serde_json::from_value(serialized).unwrap();
                prop_assert_eq!(tool.name, deserialized.name);
            }
        }

        #[test]
        fn test_any_valid_uri_passes_validation(
            scheme in "[a-zA-Z][a-zA-Z0-9+.-]*",
            path in "[a-zA-Z0-9/_.-]*"
        ) {
            let uri = format!("{}:{}", scheme, path);

            let resource = Resource {
                uri: uri.clone(),
                name: "test".to_string(),
                title: None,
                description: None,
                mime_type: None,
                annotations: None,
                meta: None,
            };

            let validator = ProtocolValidator::new();
            let result = validator.validate_resource(&resource);

            // Property: Any valid URI format should pass validation
            if uri.contains(':') && !uri.starts_with(':') {
                prop_assert!(result.is_valid() || result.is_valid_with_warnings());
            }
        }
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

fn create_minimal_tool(name: &str) -> Tool {
    Tool {
        name: name.to_string(),
        title: None,
        description: None,
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

fn create_initialize_request() -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: JsonRpcVersion::V2_0,
        id: valid_request_id(),
        method: methods::INITIALIZE.to_string(),
        params: Some(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        })),
    }
}

fn create_initialize_response() -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JsonRpcVersion::V2_0,
        id: valid_request_id(),
        result: json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }),
    }
}

fn create_list_tools_request() -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: JsonRpcVersion::V2_0,
        id: valid_request_id(),
        method: methods::LIST_TOOLS.to_string(),
        params: None,
    }
}

fn create_list_tools_response() -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JsonRpcVersion::V2_0,
        id: valid_request_id(),
        result: json!({
            "tools": []
        }),
    }
}

// =============================================================================
// Integration Tests
// =============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_full_protocol_handshake_compliance() {
        // Test complete initialization handshake per MCP spec
        let validator = ProtocolValidator::new();

        // 1. Client sends initialize request
        let init_request = create_initialize_request();
        assert!(validator.validate_request(&init_request).is_valid());

        // 2. Server responds with initialize response
        let init_response = create_initialize_response();
        assert!(validator.validate_response(&init_response).is_valid());

        // 3. Client sends initialized notification
        let initialized_notification = JsonRpcNotification {
            jsonrpc: JsonRpcVersion::V2_0,
            method: methods::INITIALIZED.to_string(),
            params: Some(json!({})),
        };
        assert!(validator.validate_notification(&initialized_notification).is_valid());
    }

    #[test]
    fn test_tool_interaction_compliance() {
        let validator = ProtocolValidator::new();

        // 1. Client requests tools list
        let list_request = create_list_tools_request();
        assert!(validator.validate_request(&list_request).is_valid());

        // 2. Server responds with tools
        let list_response = create_list_tools_response();
        assert!(validator.validate_response(&list_response).is_valid());

        // 3. Client calls a tool
        let call_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: valid_request_id(),
            method: methods::CALL_TOOL.to_string(),
            params: Some(json!({
                "name": "test_tool",
                "arguments": {}
            })),
        };
        assert!(validator.validate_request(&call_request).is_valid());
    }

    #[test]
    fn test_error_handling_compliance() {
        let validator = ProtocolValidator::new();

        // Test various error scenarios match MCP spec
        let errors = vec![
            JsonRpcError::new(error_codes::PARSE_ERROR, "Parse error".to_string()),
            JsonRpcError::new(error_codes::METHOD_NOT_FOUND, "Method not found".to_string()),
            JsonRpcError::new(crate::error_codes::TOOL_NOT_FOUND, "Tool not found".to_string()),
        ];

        for error in errors {
            let error_response = JsonRpcError {
                jsonrpc: JsonRpcVersion::V2_0,
                id: valid_request_id(),
                error: error.error,
            };

            // All error responses should be valid per spec
            let serialized = serde_json::to_value(&error_response).unwrap();
            assert!(serialized["error"]["code"].is_number());
            assert!(serialized["error"]["message"].is_string());
        }
    }
}
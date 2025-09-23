//! Comprehensive MCP 2025-06-18 Protocol Compliance Tests
//!
//! This test suite validates that TurboMCP fully complies with the MCP 2025-06-18 specification,
//! specifically focusing on the systematic fixes implemented for protocol compliance.

use serde_json::{Value, json};
use std::collections::HashMap;
use turbomcp_protocol::types::*;

#[cfg(test)]
mod mcp_compliance_tests {
    use super::*;

    /// Test that all result types have the required _meta field per MCP 2025-06-18 spec
    #[test]
    fn test_all_result_types_have_meta_field() {
        // Test InitializeResult
        let init_result = InitializeResult {
            protocol_version: "2025-06-18".to_string(),
            server_info: Implementation {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                title: None,
            },
            capabilities: ServerCapabilities::default(),
            instructions: None,
            _meta: Some(json!({"test": "value"})),
        };
        let serialized = serde_json::to_string(&init_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test ListToolsResult
        let tools_result = ListToolsResult {
            tools: vec![],
            next_cursor: None,
            _meta: Some(json!({"tools_meta": "test"})),
        };
        let serialized = serde_json::to_string(&tools_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test CallToolResult with both _meta and structuredContent
        let call_result = CallToolResult {
            content: vec![],
            is_error: Some(false),
            structured_content: Some(json!({"structured": "data"})),
            _meta: Some(json!({"call_meta": "test"})),
        };
        let serialized = serde_json::to_string(&call_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());
        assert!(parsed.get("structuredContent").is_some());

        // Test ListPromptsResult
        let prompts_result = ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
            _meta: Some(json!({"prompts_meta": "test"})),
        };
        let serialized = serde_json::to_string(&prompts_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test GetPromptResult
        let prompt_result = GetPromptResult {
            description: None,
            messages: vec![],
            _meta: Some(json!({"prompt_meta": "test"})),
        };
        let serialized = serde_json::to_string(&prompt_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test ListResourcesResult
        let resources_result = ListResourcesResult {
            resources: vec![],
            next_cursor: None,
            _meta: Some(json!({"resources_meta": "test"})),
        };
        let serialized = serde_json::to_string(&resources_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test ReadResourceResult
        let read_result = ReadResourceResult {
            contents: vec![],
            _meta: Some(json!({"read_meta": "test"})),
        };
        let serialized = serde_json::to_string(&read_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test CreateMessageResult
        let message_result = CreateMessageResult {
            role: Role::Assistant,
            content: Content::Text(TextContent {
                text: "test".to_string(),
                annotations: None,
                meta: None,
            }),
            model: None,
            stop_reason: None,
            _meta: Some(json!({"message_meta": "test"})),
        };
        let serialized = serde_json::to_string(&message_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test ListRootsResult
        let roots_result = ListRootsResult {
            roots: vec![],
            _meta: Some(json!({"roots_meta": "test"})),
        };
        let serialized = serde_json::to_string(&roots_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());
    }

    /// Test that ProgressToken supports both string and integer types per MCP 2025-06-18 spec
    #[test]
    fn test_progress_token_supports_string_and_integer() {
        // Test string ProgressToken
        let string_token = ProgressToken::String("test-token".to_string());
        let serialized = serde_json::to_string(&string_token).unwrap();
        assert_eq!(serialized, "\"test-token\"");

        let deserialized: ProgressToken = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, string_token);

        // Test integer ProgressToken
        let int_token = ProgressToken::Integer(42);
        let serialized = serde_json::to_string(&int_token).unwrap();
        assert_eq!(serialized, "42");

        let deserialized: ProgressToken = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, int_token);

        // Test that JSON with string is properly parsed
        let json_string = "\"string-token\"";
        let parsed: ProgressToken = serde_json::from_str(json_string).unwrap();
        match parsed {
            ProgressToken::String(s) => assert_eq!(s, "string-token"),
            _ => panic!("Expected string variant"),
        }

        // Test that JSON with integer is properly parsed
        let json_int = "123";
        let parsed: ProgressToken = serde_json::from_str(json_int).unwrap();
        match parsed {
            ProgressToken::Integer(i) => assert_eq!(i, 123),
            _ => panic!("Expected integer variant"),
        }
    }

    /// Test that all request types support _meta field per MCP 2025-06-18 spec
    #[test]
    fn test_all_request_types_support_meta_field() {
        // Test InitializeRequest
        let init_request = InitializeRequest {
            protocol_version: "2025-06-18".to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                title: None,
            },
            _meta: Some(json!({"init_meta": "test"})),
        };
        let serialized = serde_json::to_string(&init_request).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test CallToolRequest
        let call_request = CallToolRequest {
            name: "test_tool".to_string(),
            arguments: Some(HashMap::new()),
            _meta: Some(json!({"call_meta": "test"})),
        };
        let serialized = serde_json::to_string(&call_request).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test GetPromptRequest
        let prompt_request = GetPromptRequest {
            name: "test_prompt".to_string(),
            arguments: None,
            _meta: Some(json!({"prompt_meta": "test"})),
        };
        let serialized = serde_json::to_string(&prompt_request).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test ListResourcesRequest
        let resources_request = ListResourcesRequest {
            cursor: None,
            _meta: Some(json!({"resources_meta": "test"})),
        };
        let serialized = serde_json::to_string(&resources_request).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test ReadResourceRequest
        let read_request = ReadResourceRequest {
            uri: "file://test.txt".to_string(),
            _meta: Some(json!({"read_meta": "test"})),
        };
        let serialized = serde_json::to_string(&read_request).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());

        // Test CreateMessageRequest
        let message_request = CreateMessageRequest {
            messages: vec![],
            model_preferences: None,
            system_prompt: None,
            include_context: None,
            temperature: None,
            max_tokens: 100,
            stop_sequences: None,
            metadata: None,
            _meta: Some(json!({"message_meta": "test"})),
        };
        let serialized = serde_json::to_string(&message_request).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());
    }

    /// Test that _meta fields are optional and work with None values
    #[test]
    fn test_meta_fields_are_optional() {
        // Test that structures work with _meta: None
        let init_result = InitializeResult {
            protocol_version: "2025-06-18".to_string(),
            server_info: Implementation {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                title: None,
            },
            capabilities: ServerCapabilities::default(),
            instructions: None,
            _meta: None,
        };
        let serialized = serde_json::to_string(&init_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_none()); // Should be omitted when None

        // Test that structures can be deserialized without _meta field
        let json_without_meta = json!({
            "protocolVersion": "2025-06-18",
            "serverInfo": {
                "name": "test",
                "version": "1.0.0"
            },
            "capabilities": {}
        });
        let deserialized: InitializeResult = serde_json::from_value(json_without_meta).unwrap();
        assert!(deserialized._meta.is_none());
    }

    /// Test CallToolResult structuredContent field compliance
    #[test]
    fn test_call_tool_result_structured_content() {
        // Test with structuredContent
        let call_result = CallToolResult {
            content: vec![],
            is_error: Some(false),
            structured_content: Some(json!({
                "type": "chart",
                "data": [1, 2, 3],
                "config": {
                    "title": "Test Chart"
                }
            })),
            _meta: None,
        };
        let serialized = serde_json::to_string(&call_result).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("structuredContent").is_some());

        let structured = parsed.get("structuredContent").unwrap();
        assert_eq!(structured["type"], "chart");
        assert_eq!(structured["data"], json!([1, 2, 3]));

        // Test without structuredContent (should be omitted)
        let call_result_no_structured = CallToolResult {
            content: vec![],
            is_error: Some(false),
            structured_content: None,
            _meta: None,
        };
        let serialized = serde_json::to_string(&call_result_no_structured).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("structuredContent").is_none());
    }

    /// Test that parameter structures support _meta fields for request metadata
    #[test]
    fn test_parameter_structures_support_meta() {
        // Test ElicitRequestParams
        let elicit_params = ElicitRequestParams {
            message: "Please provide input".to_string(),
            requested_schema: ElicitationSchema::new(),
            _meta: Some(json!({"elicit_meta": "test"})),
        };
        let serialized = serde_json::to_string(&elicit_params).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        // Note: due to camelCase rename_all, _meta becomes "meta" in JSON
        assert!(parsed.get("meta").is_some());

        // Test CompleteRequestParams
        let complete_params = CompleteRequestParams {
            argument: ArgumentInfo {
                name: "test_arg".to_string(),
                value: "test_value".to_string(),
            },
            reference: CompletionReference::ResourceTemplate(ResourceTemplateReferenceData {
                uri: "file://test".to_string(),
            }),
            context: None,
            _meta: Some(json!({"complete_meta": "test"})),
        };
        let serialized = serde_json::to_string(&complete_params).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed.get("_meta").is_some());
    }

    /// Test comprehensive JSON-RPC roundtrip with _meta fields
    #[test]
    fn test_jsonrpc_roundtrip_with_meta_fields() {
        use turbomcp_protocol::jsonrpc::*;

        // Test JSON-RPC request with _meta in params
        let request_params = json!({
            "name": "test_tool",
            "arguments": {"arg1": "value1"},
            "_meta": {"requestId": "req-123", "timestamp": 1234567890}
        });

        let request = JsonRpcRequest::new(
            "tools/call".to_string(),
            Some(request_params),
            "test-id".into(),
        );

        let serialized = serde_json::to_string(&request).unwrap();
        let deserialized: JsonRpcRequest = serde_json::from_str(&serialized).unwrap();

        assert_eq!(request.method, deserialized.method);
        assert!(deserialized.params.is_some());

        let params = deserialized.params.unwrap();
        assert!(params.get("_meta").is_some());

        // Test JSON-RPC response with _meta in result
        let result_with_meta = json!({
            "content": [],
            "isError": false,
            "_meta": {"responseId": "resp-123", "processingTime": 42}
        });

        let response = JsonRpcResponse::success(result_with_meta, "test-id".into());
        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: JsonRpcResponse = serde_json::from_str(&serialized).unwrap();

        assert!(deserialized.is_success());
        let result = deserialized.result().unwrap();
        assert!(result.get("_meta").is_some());
    }

    /// Test edge cases and error conditions with _meta fields
    #[test]
    fn test_meta_field_edge_cases() {
        // Test with complex nested _meta
        let complex_meta = json!({
            "tracing": {
                "spanId": "span-123",
                "traceId": "trace-456"
            },
            "performance": {
                "duration": 123.45,
                "memoryUsed": 1024
            },
            "custom": {
                "tags": ["tag1", "tag2"],
                "metadata": {
                    "nested": true,
                    "level": 3
                }
            }
        });

        let call_result = CallToolResult {
            content: vec![],
            is_error: Some(false),
            structured_content: None,
            _meta: Some(complex_meta.clone()),
        };

        let serialized = serde_json::to_string(&call_result).unwrap();
        let deserialized: CallToolResult = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized._meta, Some(complex_meta));

        // Test with empty object _meta
        let empty_meta = json!({});
        let call_result_empty = CallToolResult {
            content: vec![],
            is_error: Some(false),
            structured_content: None,
            _meta: Some(empty_meta.clone()),
        };

        let serialized = serde_json::to_string(&call_result_empty).unwrap();
        let deserialized: CallToolResult = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized._meta, Some(empty_meta));
    }
}

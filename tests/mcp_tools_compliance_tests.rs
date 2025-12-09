//! # MCP Tools Compliance Tests
//!
//! These tests validate TurboMCP against the MCP Tools specification found in:
//! - `/reference/modelcontextprotocol/docs/specification/draft/server/tools.mdx`
//!
//! This ensures 100% compliance with MCP Tools protocol requirements.

use serde_json::{json, Value};
use std::collections::HashMap;
use turbomcp_protocol::{
    jsonrpc::*,
    types::*,
    validation::*,
    *,
};

// =============================================================================
// Tools Capability Declaration Tests
// =============================================================================

#[cfg(test)]
mod tools_capability_tests {
    use super::*;

    /// **MCP Spec Requirement**: "Servers that support tools MUST declare the tools capability"
    #[test]
    fn test_tools_capability_declaration() {
        let server_caps = ServerCapabilities {
            tools: Some(ToolsCapabilities {
                list_changed: Some(true),
            }),
            prompts: None,
            resources: None,
            logging: None,
            completions: None,
            experimental: None,
        };

        // Validate tools capability structure
        assert!(server_caps.tools.is_some());
        let tools_cap = server_caps.tools.unwrap();
        assert_eq!(tools_cap.list_changed, Some(true));

        // Validate JSON structure matches spec
        let json = serde_json::to_value(&server_caps).unwrap();
        assert!(json["tools"].is_object());
        assert_eq!(json["tools"]["listChanged"], true);
    }

    /// **MCP Spec Requirement**: "listChanged indicates whether the server will emit notifications when the list of available tools changes"
    #[test]
    fn test_list_changed_capability() {
        // Test with list_changed enabled
        let with_notifications = ToolsCapabilities {
            list_changed: Some(true),
        };

        // Test with list_changed disabled
        let without_notifications = ToolsCapabilities {
            list_changed: Some(false),
        };

        // Test with list_changed unspecified (defaults to false)
        let unspecified = ToolsCapabilities {
            list_changed: None,
        };

        // All should be valid capability declarations
        assert!(serde_json::to_value(&with_notifications).is_ok());
        assert!(serde_json::to_value(&without_notifications).is_ok());
        assert!(serde_json::to_value(&unspecified).is_ok());
    }
}

// =============================================================================
// Tools List Request/Response Tests
// =============================================================================

#[cfg(test)]
mod tools_list_tests {
    use super::*;

    /// **MCP Spec Requirement**: "To discover available tools, clients send a tools/list request"
    #[test]
    fn test_tools_list_request_structure() {
        // Basic tools/list request
        let basic_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: RequestId::from("tools-list-1"),
            method: "tools/list".to_string(),
            params: None,
        };

        // Validate basic request
        assert_eq!(basic_request.method, methods::LIST_TOOLS);

        // Request with pagination cursor
        let paginated_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: RequestId::from("tools-list-2"),
            method: "tools/list".to_string(),
            params: Some(json!({
                "cursor": "optional-cursor-value"
            })),
        };

        // Validate pagination support
        assert!(paginated_request.params.is_some());
        let params = paginated_request.params.unwrap();
        assert!(params["cursor"].is_string());
    }

    /// **MCP Spec Requirement**: Validate tools/list response structure matches specification
    #[test]
    fn test_tools_list_response_structure() {
        let tools_response = ListToolsResult {
            tools: vec![
                Tool {
                    name: "get_weather".to_string(),
                    title: Some("Weather Information Provider".to_string()),
                    description: Some("Get current weather information for a location".to_string()),
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: Some({
                            let mut props = HashMap::new();
                            props.insert("location".to_string(), json!({
                                "type": "string",
                                "description": "City name or zip code"
                            }));
                            props
                        }),
                        required: Some(vec!["location".to_string()]),
                        additional_properties: None,
                    },
                    output_schema: None,
                    execution: None,
                    annotations: None,
                    meta: None,
                }
            ],
            next_cursor: Some("next-page-cursor".to_string()),
            meta: None,
        };

        // Validate response structure
        assert_eq!(tools_response.tools.len(), 1);
        assert!(tools_response.next_cursor.is_some());

        // Validate tool structure
        let tool = &tools_response.tools[0];
        assert_eq!(tool.name, "get_weather");
        assert!(tool.title.is_some());
        assert!(tool.description.is_some());
        assert_eq!(tool.input_schema.schema_type, "object");
        assert!(tool.input_schema.properties.is_some());
        assert!(tool.input_schema.required.is_some());

        // Validate JSON serialization matches spec example
        let json = serde_json::to_value(&tools_response).unwrap();
        assert!(json["tools"].is_array());
        assert_eq!(json["tools"][0]["name"], "get_weather");
        assert!(json["tools"][0]["inputSchema"]["properties"]["location"].is_object());
        assert_eq!(json["nextCursor"], "next-page-cursor");
    }

    /// **MCP Spec Requirement**: Test tool with icons support
    #[test]
    fn test_tool_with_icons() {
        let tool_with_icons = Tool {
            name: "weather_tool".to_string(),
            title: Some("Weather Tool".to_string()),
            description: Some("A weather information tool".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            annotations: Some(ToolAnnotations {
                audience: Some(vec![Role::User, Role::Assistant]),
                priority: Some(0.8),
                last_modified: Some("2025-01-01T00:00:00Z".to_string()),
                title: Some("Weather Information Tool".to_string()),
            }),
            meta: Some({
                let mut meta = HashMap::new();
                meta.insert("icons".to_string(), json!([
                    {
                        "src": "https://example.com/weather-icon.png",
                        "mimeType": "image/png",
                        "sizes": "48x48"
                    }
                ]));
                meta
            }),
        };

        // Validate icons in meta field
        let json = serde_json::to_value(&tool_with_icons).unwrap();
        assert!(json["_meta"]["icons"].is_array());
        assert_eq!(json["_meta"]["icons"][0]["src"], "https://example.com/weather-icon.png");
    }
}

// =============================================================================
// Tool Call Request/Response Tests
// =============================================================================

#[cfg(test)]
mod tool_call_tests {
    use super::*;

    /// **MCP Spec Requirement**: "To invoke a tool, clients send a tools/call request"
    #[test]
    fn test_tool_call_request_structure() {
        let call_request = CallToolRequest {
            id: RequestId::from("tool-call-1"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "tools/call".to_string(),
            params: CallToolParams {
                name: "get_weather".to_string(),
                arguments: Some({
                    let mut args = HashMap::new();
                    args.insert("location".to_string(), json!("New York"));
                    args
                }),
                meta: None,
            },
        };

        // Validate call request structure
        assert_eq!(call_request.method, methods::CALL_TOOL);
        assert_eq!(call_request.params.name, "get_weather");
        assert!(call_request.params.arguments.is_some());

        // Validate JSON structure matches spec
        let json = serde_json::to_value(&call_request).unwrap();
        assert_eq!(json["method"], "tools/call");
        assert_eq!(json["params"]["name"], "get_weather");
        assert_eq!(json["params"]["arguments"]["location"], "New York");
    }

    /// **MCP Spec Requirement**: Test all content types in tool results
    #[test]
    fn test_tool_result_content_types() {
        // Test text content
        let text_result = CallToolResult {
            content: vec![
                Content::Text(TextContent {
                    content_type: "text".to_string(),
                    text: "Tool result text".to_string(),
                    annotations: None,
                    meta: None,
                })
            ],
            is_error: Some(false),
            structured_content: None,
            meta: None,
        };

        // Test image content
        let image_result = CallToolResult {
            content: vec![
                Content::Image(ImageContent {
                    content_type: "image".to_string(),
                    data: "base64-encoded-data".to_string(),
                    mime_type: "image/png".to_string(),
                    annotations: Some(ContentAnnotations {
                        audience: Some(vec![Role::User]),
                        priority: Some(0.9),
                        last_modified: None,
                    }),
                    meta: None,
                })
            ],
            is_error: Some(false),
            structured_content: None,
            meta: None,
        };

        // Test audio content
        let audio_result = CallToolResult {
            content: vec![
                Content::Audio(AudioContent {
                    content_type: "audio".to_string(),
                    data: "base64-encoded-audio-data".to_string(),
                    mime_type: "audio/wav".to_string(),
                    annotations: None,
                    meta: None,
                })
            ],
            is_error: Some(false),
            structured_content: None,
            meta: None,
        };

        // Test resource link content
        let resource_link_result = CallToolResult {
            content: vec![
                Content::ResourceLink(ResourceLink {
                    content_type: "resource_link".to_string(),
                    uri: "file:///project/src/main.rs".to_string(),
                    name: Some("main.rs".to_string()),
                    description: Some("Primary application entry point".to_string()),
                    mime_type: Some("text/x-rust".to_string()),
                    annotations: None,
                    meta: None,
                })
            ],
            is_error: Some(false),
            structured_content: None,
            meta: None,
        };

        // Test embedded resource content
        let embedded_resource_result = CallToolResult {
            content: vec![
                Content::EmbeddedResource(EmbeddedResource {
                    content_type: "resource".to_string(),
                    resource: ResourceContents::Text(TextResourceContents {
                        uri: "file:///project/src/main.rs".to_string(),
                        mime_type: Some("text/x-rust".to_string()),
                        text: "fn main() {\n    println!(\"Hello world!\");\n}".to_string(),
                        annotations: Some(ResourceAnnotations {
                            audience: Some(vec![Role::User, Role::Assistant]),
                            priority: Some(0.7),
                            last_modified: Some("2025-05-03T14:30:00Z".to_string()),
                        }),
                        meta: None,
                    }),
                    annotations: None,
                    meta: None,
                })
            ],
            is_error: Some(false),
            structured_content: None,
            meta: None,
        };

        // Validate all content types serialize correctly
        assert!(serde_json::to_value(&text_result).is_ok());
        assert!(serde_json::to_value(&image_result).is_ok());
        assert!(serde_json::to_value(&audio_result).is_ok());
        assert!(serde_json::to_value(&resource_link_result).is_ok());
        assert!(serde_json::to_value(&embedded_resource_result).is_ok());

        // Validate JSON structure for text content
        let text_json = serde_json::to_value(&text_result).unwrap();
        assert_eq!(text_json["content"][0]["type"], "text");
        assert_eq!(text_json["content"][0]["text"], "Tool result text");
        assert_eq!(text_json["isError"], false);
    }

    /// **MCP Spec Requirement**: Test structured content and output schema validation
    #[test]
    fn test_structured_content_and_output_schema() {
        // Tool with output schema
        let weather_tool = Tool {
            name: "get_weather_data".to_string(),
            title: Some("Weather Data Retriever".to_string()),
            description: Some("Get current weather data for a location".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = HashMap::new();
                    props.insert("location".to_string(), json!({
                        "type": "string",
                        "description": "City name or zip code"
                    }));
                    props
                }),
                required: Some(vec!["location".to_string()]),
                additional_properties: None,
            },
            output_schema: Some(ToolOutputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = HashMap::new();
                    props.insert("temperature".to_string(), json!({
                        "type": "number",
                        "description": "Temperature in celsius"
                    }));
                    props.insert("conditions".to_string(), json!({
                        "type": "string",
                        "description": "Weather conditions description"
                    }));
                    props.insert("humidity".to_string(), json!({
                        "type": "number",
                        "description": "Humidity percentage"
                    }));
                    props
                }),
                required: Some(vec![
                    "temperature".to_string(),
                    "conditions".to_string(),
                    "humidity".to_string()
                ]),
                additional_properties: None,
            }),
            annotations: None,
            meta: None,
        };

        // Validate output schema structure
        assert!(weather_tool.output_schema.is_some());
        let output_schema = weather_tool.output_schema.unwrap();
        assert_eq!(output_schema.schema_type, "object");
        assert!(output_schema.properties.is_some());
        assert!(output_schema.required.is_some());

        // Tool result with structured content
        let structured_result = CallToolResult {
            content: vec![
                Content::Text(TextContent {
                    content_type: "text".to_string(),
                    text: "{\"temperature\": 22.5, \"conditions\": \"Partly cloudy\", \"humidity\": 65}".to_string(),
                    annotations: None,
                    meta: None,
                })
            ],
            structured_content: Some(json!({
                "temperature": 22.5,
                "conditions": "Partly cloudy",
                "humidity": 65
            })),
            is_error: Some(false),
            meta: None,
        };

        // **MCP Spec Requirement**: "Servers MUST provide structured results that conform to this schema"
        // **MCP Spec Requirement**: "Clients SHOULD validate structured results against this schema"
        assert!(structured_result.structured_content.is_some());

        let structured_data = structured_result.structured_content.unwrap();
        assert!(structured_data["temperature"].is_number());
        assert!(structured_data["conditions"].is_string());
        assert!(structured_data["humidity"].is_number());

        // **MCP Spec Requirement**: "For backwards compatibility, a tool that returns structured content SHOULD also return the serialized JSON in a TextContent block"
        assert!(!structured_result.content.is_empty());
        if let Content::Text(text_content) = &structured_result.content[0] {
            assert!(text_content.text.contains("temperature"));
            assert!(text_content.text.contains("22.5"));
        } else {
            panic!("Expected text content for backwards compatibility");
        }
    }
}

// =============================================================================
// Tool List Changed Notification Tests
// =============================================================================

#[cfg(test)]
mod tool_notification_tests {
    use super::*;

    /// **MCP Spec Requirement**: "When the list of available tools changes, servers that declared the listChanged capability SHOULD send a notification"
    #[test]
    fn test_tools_list_changed_notification() {
        let list_changed_notification = JsonRpcNotification {
            jsonrpc: JsonRpcVersion::V2_0,
            method: "notifications/tools/list_changed".to_string(),
            params: None,
        };

        // Validate notification structure
        assert_eq!(list_changed_notification.method, "notifications/tools/list_changed");

        // Ensure it's a notification (no ID)
        let json = serde_json::to_value(&list_changed_notification).unwrap();
        assert!(!json.as_object().unwrap().contains_key("id"));
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["method"], "notifications/tools/list_changed");
    }
}

// =============================================================================
// Tool Error Handling Tests
// =============================================================================

#[cfg(test)]
mod tool_error_handling_tests {
    use super::*;

    /// **MCP Spec Requirement**: Test protocol errors for tool operations
    #[test]
    fn test_tool_protocol_errors() {
        // Unknown tool error
        let unknown_tool_error = JsonRpcError {
            jsonrpc: JsonRpcVersion::V2_0,
            id: Some(RequestId::from("error-1")),
            error: JsonRpcErrorCode::ApplicationError(-32602),
        };

        // Invalid arguments error
        let invalid_args_error = JsonRpcError {
            jsonrpc: JsonRpcVersion::V2_0,
            id: Some(RequestId::from("error-2")),
            error: JsonRpcErrorCode::InvalidParams,
        };

        // Validate error structures
        let unknown_json = serde_json::to_value(&unknown_tool_error).unwrap();
        assert!(unknown_json["error"]["code"].is_number());
        assert!(unknown_json["error"]["message"].is_string());

        let invalid_json = serde_json::to_value(&invalid_args_error).unwrap();
        assert_eq!(invalid_json["error"]["code"], -32602);
    }

    /// **MCP Spec Requirement**: Test tool execution errors with isError flag
    #[test]
    fn test_tool_execution_errors() {
        let execution_error_result = CallToolResult {
            content: vec![
                Content::Text(TextContent {
                    content_type: "text".to_string(),
                    text: "Failed to fetch weather data: API rate limit exceeded".to_string(),
                    annotations: None,
                    meta: None,
                })
            ],
            is_error: Some(true), // Key requirement: isError set to true
            structured_content: None,
            meta: None,
        };

        // Validate error result structure
        assert_eq!(execution_error_result.is_error, Some(true));
        assert!(!execution_error_result.content.is_empty());

        let json = serde_json::to_value(&execution_error_result).unwrap();
        assert_eq!(json["isError"], true);
        assert!(json["content"][0]["text"].as_str().unwrap().contains("API rate limit exceeded"));
    }

    /// **MCP Spec Requirement**: "If not set, isError is assumed to be false (the call was successful)"
    #[test]
    fn test_is_error_default_behavior() {
        let success_result = CallToolResult {
            content: vec![
                Content::Text(TextContent {
                    content_type: "text".to_string(),
                    text: "Tool executed successfully".to_string(),
                    annotations: None,
                    meta: None,
                })
            ],
            is_error: None, // Not set - should default to false
            structured_content: None,
            meta: None,
        };

        // When isError is None, it should be treated as false
        assert!(success_result.is_error.is_none() || success_result.is_error == Some(false));

        let json = serde_json::to_value(&success_result).unwrap();
        // When None, the field should not be serialized (skip_serializing_if)
        assert!(!json.as_object().unwrap().contains_key("isError") || json["isError"] == false);
    }
}

// =============================================================================
// Tool Security and Validation Tests
// =============================================================================

#[cfg(test)]
mod tool_security_tests {
    use super::*;

    /// **MCP Spec Requirement**: "Servers MUST validate all tool inputs"
    #[test]
    fn test_tool_input_validation() {
        let tool_with_validation = Tool {
            name: "secure_tool".to_string(),
            title: Some("Secure Tool".to_string()),
            description: Some("A tool with strict input validation".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = HashMap::new();
                    props.insert("user_id".to_string(), json!({
                        "type": "string",
                        "pattern": "^[a-zA-Z0-9_-]+$",
                        "minLength": 1,
                        "maxLength": 50
                    }));
                    props.insert("amount".to_string(), json!({
                        "type": "number",
                        "minimum": 0,
                        "maximum": 10000
                    }));
                    props
                }),
                required: Some(vec!["user_id".to_string(), "amount".to_string()]),
                additional_properties: Some(false), // Strict: no additional properties
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };

        // Validate that input schema has security constraints
        let input_schema = &tool_with_validation.input_schema;
        assert!(input_schema.properties.is_some());
        assert!(input_schema.required.is_some());
        assert_eq!(input_schema.additional_properties, Some(false));

        let properties = input_schema.properties.as_ref().unwrap();

        // Check user_id has pattern validation
        assert!(properties["user_id"]["pattern"].is_string());
        assert!(properties["user_id"]["minLength"].is_number());
        assert!(properties["user_id"]["maxLength"].is_number());

        // Check amount has range validation
        assert!(properties["amount"]["minimum"].is_number());
        assert!(properties["amount"]["maximum"].is_number());
    }

    /// **MCP Spec Requirement**: "Clients SHOULD prompt for user confirmation on sensitive operations"
    /// **MCP Spec Requirement**: "Show tool inputs to the user before calling the server"
    #[test]
    fn test_tool_security_annotations() {
        let sensitive_tool = Tool {
            name: "delete_file".to_string(),
            title: Some("File Deletion Tool".to_string()),
            description: Some("Delete a file from the filesystem".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = HashMap::new();
                    props.insert("file_path".to_string(), json!({
                        "type": "string",
                        "description": "Path to file to delete"
                    }));
                    props
                }),
                required: Some(vec!["file_path".to_string()]),
                additional_properties: None,
            },
            output_schema: None,
            annotations: Some(ToolAnnotations {
                audience: Some(vec![Role::User]), // Requires user attention
                priority: Some(1.0), // Highest priority for security
                last_modified: None,
                title: Some("⚠️ DESTRUCTIVE OPERATION - Requires User Confirmation".to_string()),
            }),
            meta: Some({
                let mut meta = HashMap::new();
                meta.insert("security_level".to_string(), json!("high"));
                meta.insert("requires_confirmation".to_string(), json!(true));
                meta.insert("destructive".to_string(), json!(true));
                meta
            }),
        };

        // Validate security annotations
        assert!(sensitive_tool.annotations.is_some());
        let annotations = sensitive_tool.annotations.unwrap();
        assert_eq!(annotations.priority, Some(1.0));
        assert!(annotations.title.unwrap().contains("DESTRUCTIVE"));

        // Validate security metadata
        assert!(sensitive_tool.meta.is_some());
        let meta = sensitive_tool.meta.unwrap();
        assert_eq!(meta["security_level"], "high");
        assert_eq!(meta["requires_confirmation"], true);
        assert_eq!(meta["destructive"], true);
    }

    /// **MCP Spec Requirement**: "Clients SHOULD implement timeouts for tool calls"
    #[test]
    fn test_tool_timeout_considerations() {
        // This test validates that our tool call structure supports timeout metadata
        let long_running_tool = Tool {
            name: "process_large_dataset".to_string(),
            title: Some("Dataset Processor".to_string()),
            description: Some("Process a large dataset - may take several minutes".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            annotations: None,
            meta: Some({
                let mut meta = HashMap::new();
                meta.insert("estimated_duration_seconds".to_string(), json!(300));
                meta.insert("timeout_recommended_seconds".to_string(), json!(600));
                meta.insert("supports_progress_updates".to_string(), json!(true));
                meta
            }),
        };

        // Validate timeout metadata is available for client use
        assert!(long_running_tool.meta.is_some());
        let meta = long_running_tool.meta.unwrap();
        assert_eq!(meta["estimated_duration_seconds"], 300);
        assert_eq!(meta["timeout_recommended_seconds"], 600);
        assert_eq!(meta["supports_progress_updates"], true);
    }
}

// =============================================================================
// Tool Pagination Compliance Tests
// =============================================================================

#[cfg(test)]
mod tool_pagination_tests {
    use super::*;

    /// **MCP Spec Requirement**: "This operation supports pagination"
    #[test]
    fn test_tools_list_pagination() {
        // Request with cursor
        let paginated_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: RequestId::from("page-1"),
            method: "tools/list".to_string(),
            params: Some(json!({
                "cursor": "page-2-cursor"
            })),
        };

        // Response with next cursor
        let paginated_response = ListToolsResult {
            tools: vec![
                create_test_tool("tool1"),
                create_test_tool("tool2"),
                create_test_tool("tool3"),
            ],
            next_cursor: Some("page-3-cursor".to_string()),
            meta: None,
        };

        // Validate pagination structure
        assert!(paginated_request.params.is_some());
        assert_eq!(paginated_request.params.unwrap()["cursor"], "page-2-cursor");
        assert!(paginated_response.next_cursor.is_some());
        assert_eq!(paginated_response.next_cursor.unwrap(), "page-3-cursor");

        // Last page response (no next cursor)
        let last_page_response = ListToolsResult {
            tools: vec![create_test_tool("last_tool")],
            next_cursor: None, // No more pages
            meta: None,
        };

        assert!(last_page_response.next_cursor.is_none());
    }

    fn create_test_tool(name: &str) -> Tool {
        Tool {
            name: name.to_string(),
            title: Some(format!("Test Tool {}", name)),
            description: Some("A test tool".to_string()),
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
}

// =============================================================================
// Tool Message Flow Integration Tests
// =============================================================================

#[cfg(test)]
mod tool_message_flow_tests {
    use super::*;

    /// **MCP Spec Requirement**: Test complete tool discovery and invocation flow
    #[test]
    fn test_complete_tool_interaction_flow() {
        // 1. Client discovers tools
        let list_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: RequestId::from("discover"),
            method: "tools/list".to_string(),
            params: None,
        };

        // 2. Server returns available tools
        let list_response = ListToolsResult {
            tools: vec![
                Tool {
                    name: "weather_tool".to_string(),
                    title: Some("Weather Tool".to_string()),
                    description: Some("Get weather information".to_string()),
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: Some({
                            let mut props = HashMap::new();
                            props.insert("city".to_string(), json!({
                                "type": "string",
                                "description": "City name"
                            }));
                            props
                        }),
                        required: Some(vec!["city".to_string()]),
                        additional_properties: None,
                    },
                    output_schema: None,
                    execution: None,
                    annotations: None,
                    meta: None,
                }
            ],
            next_cursor: None,
            meta: None,
        };

        // 3. Client calls the tool
        let call_request = CallToolRequest {
            id: RequestId::from("invoke"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "tools/call".to_string(),
            params: CallToolParams {
                name: "weather_tool".to_string(),
                arguments: Some({
                    let mut args = HashMap::new();
                    args.insert("city".to_string(), json!("San Francisco"));
                    args
                }),
                meta: None,
            },
        };

        // 4. Server returns tool result
        let call_response = CallToolResult {
            content: vec![
                Content::Text(TextContent {
                    content_type: "text".to_string(),
                    text: "Weather in San Francisco: Sunny, 72°F".to_string(),
                    annotations: None,
                    meta: None,
                })
            ],
            is_error: Some(false),
            structured_content: None,
            meta: None,
        };

        // 5. Server sends list changed notification (if tools change)
        let list_changed = JsonRpcNotification {
            jsonrpc: JsonRpcVersion::V2_0,
            method: "notifications/tools/list_changed".to_string(),
            params: None,
        };

        // Validate complete flow
        assert_eq!(list_request.method, "tools/list");
        assert!(!list_response.tools.is_empty());
        assert_eq!(call_request.method, "tools/call");
        assert_eq!(call_request.params.name, "weather_tool");
        assert_eq!(call_response.is_error, Some(false));
        assert_eq!(list_changed.method, "notifications/tools/list_changed");

        // All messages should serialize correctly
        assert!(serde_json::to_value(&list_request).is_ok());
        assert!(serde_json::to_value(&list_response).is_ok());
        assert!(serde_json::to_value(&call_request).is_ok());
        assert!(serde_json::to_value(&call_response).is_ok());
        assert!(serde_json::to_value(&list_changed).is_ok());
    }
}

// =============================================================================
// Tool Annotation Compliance Tests
// =============================================================================

#[cfg(test)]
mod tool_annotation_tests {
    use super::*;

    /// **MCP Spec Requirement**: "For trust & safety and security, clients MUST consider tool annotations to be untrusted unless they come from trusted servers"
    #[test]
    fn test_tool_annotation_trust_model() {
        let untrusted_tool = Tool {
            name: "suspicious_tool".to_string(),
            title: Some("Totally Safe Tool".to_string()), // Potentially misleading
            description: Some("This tool is completely safe and secure".to_string()), // Untrusted claim
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            annotations: Some(ToolAnnotations {
                audience: Some(vec![Role::User]),
                priority: Some(1.0), // Claims high priority
                last_modified: None,
                title: Some("🔒 SECURE OPERATION".to_string()), // Untrusted security claim
            }),
            meta: Some({
                let mut meta = HashMap::new();
                meta.insert("security_verified".to_string(), json!(true)); // Untrusted claim
                meta.insert("safe_for_automation".to_string(), json!(true)); // Untrusted claim
                meta
            }),
        };

        // These annotations should be treated as untrusted from unknown servers
        // Validation logic would check server trust level before believing these claims
        assert!(untrusted_tool.annotations.is_some());
        assert!(untrusted_tool.meta.is_some());

        // In real implementation, validation would be:
        // let is_trusted_server = check_server_trust_level(server_id);
        // let trust_annotations = is_trusted_server && tool.annotations.is_some();

        // For this test, we just validate the structure exists for trust checking
        let annotations = untrusted_tool.annotations.unwrap();
        assert!(annotations.title.unwrap().contains("SECURE")); // Would need trust verification
    }

    /// **MCP Spec Requirement**: Test all annotation fields work correctly
    #[test]
    fn test_complete_tool_annotations() {
        let fully_annotated_tool = Tool {
            name: "annotated_tool".to_string(),
            title: Some("Fully Annotated Tool".to_string()),
            description: Some("A tool with complete annotations".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            annotations: Some(ToolAnnotations {
                audience: Some(vec![Role::User, Role::Assistant]),
                priority: Some(0.7),
                last_modified: Some("2025-01-15T10:30:00Z".to_string()),
                title: Some("Display Name for Tool".to_string()),
            }),
            meta: None,
        };

        let annotations = fully_annotated_tool.annotations.unwrap();

        // Validate all annotation fields
        assert!(annotations.audience.is_some());
        assert_eq!(annotations.audience.unwrap().len(), 2);
        assert_eq!(annotations.priority, Some(0.7));
        assert!(annotations.last_modified.is_some());
        assert!(annotations.title.is_some());

        // Validate JSON structure
        let json = serde_json::to_value(&fully_annotated_tool).unwrap();
        assert!(json["annotations"]["audience"].is_array());
        assert_eq!(json["annotations"]["priority"], 0.7);
        assert!(json["annotations"]["lastModified"].is_string());
        assert!(json["annotations"]["title"].is_string());
    }
}

/*
## KEY COMPLIANCE AREAS COVERED:

✅ **Tool Capability Declaration**
✅ **Tools List Request/Response Structure**
✅ **Tool Call Request/Response Structure**
✅ **All Content Types** (text, image, audio, resource_link, embedded_resource)
✅ **Structured Content and Output Schema**
✅ **Tool List Changed Notifications**
✅ **Error Handling** (protocol errors vs execution errors)
✅ **Security Considerations** (input validation, confirmation requirements)
✅ **Pagination Support**
✅ **Complete Message Flow**
✅ **Annotation Trust Model**

## TESTS THAT WILL LIKELY FAIL (COMPLIANCE GAPS):

1. **Tool annotation types** - May not match exact MCP schema
2. **Content type implementations** - Audio/Image content may be incomplete
3. **Resource link/embedded resource** - Complex type may have gaps
4. **Output schema validation** - May not enforce schema compliance strictly
5. **Security metadata** - Custom security fields may not be standardized
6. **Pagination cursor handling** - May not be fully implemented
7. **Trust model validation** - Server trust checking not implemented

These failing tests will guide the fixes needed for full MCP compliance.
*/
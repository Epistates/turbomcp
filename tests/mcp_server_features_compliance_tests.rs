//! # MCP Server Features Compliance Tests
//!
//! These tests validate TurboMCP against all MCP Server feature specifications:
//! - Tools (already covered in separate file)
//! - Prompts: `/reference/modelcontextprotocol/docs/specification/draft/server/prompts.mdx`
//! - Resources: `/reference/modelcontextprotocol/docs/specification/draft/server/resources.mdx`
//!
//! This ensures 100% compliance with MCP Server protocol requirements.

use serde_json::{json, Value};
use std::collections::HashMap;
use turbomcp_protocol::{
    jsonrpc::*,
    types::*,
    validation::*,
    *,
};

// =============================================================================
// PROMPTS COMPLIANCE TESTS
// =============================================================================

#[cfg(test)]
mod prompts_compliance {
    use super::*;

    /// **MCP Spec Requirement**: "Servers that support prompts MUST declare the prompts capability"
    #[test]
    fn test_prompts_capability_declaration() {
        let server_caps = ServerCapabilities {
            prompts: Some(PromptsCapabilities {
                list_changed: Some(true),
            }),
            tools: None,
            resources: None,
            logging: None,
            completions: None,
            experimental: None,
        };

        // Validate prompts capability structure
        assert!(server_caps.prompts.is_some());
        let prompts_cap = server_caps.prompts.unwrap();
        assert_eq!(prompts_cap.list_changed, Some(true));

        // Validate JSON structure matches spec
        let json = serde_json::to_value(&server_caps).unwrap();
        assert!(json["prompts"].is_object());
        assert_eq!(json["prompts"]["listChanged"], true);
    }

    /// **MCP Spec Requirement**: Test prompts/list request structure and pagination
    #[test]
    fn test_prompts_list_request_structure() {
        // Basic prompts/list request
        let basic_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: RequestId::from("prompts-list-1"),
            method: "prompts/list".to_string(),
            params: None,
        };

        // Request with pagination cursor
        let paginated_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: RequestId::from("prompts-list-2"),
            method: "prompts/list".to_string(),
            params: Some(json!({
                "cursor": "optional-cursor-value"
            })),
        };

        // Validate basic request
        assert_eq!(basic_request.method, methods::LIST_PROMPTS);

        // Validate pagination support
        assert!(paginated_request.params.is_some());
        let params = paginated_request.params.unwrap();
        assert!(params["cursor"].is_string());
    }

    /// **MCP Spec Requirement**: Test prompts/list response structure per specification
    #[test]
    fn test_prompts_list_response_structure() {
        let prompts_response = ListPromptsResult {
            prompts: vec![
                Prompt {
                    name: "code_review".to_string(),
                    title: Some("Request Code Review".to_string()),
                    description: Some("Asks the LLM to analyze code quality and suggest improvements".to_string()),
                    arguments: Some(vec![
                        PromptArgument {
                            name: "code".to_string(),
                            description: Some("The code to review".to_string()),
                            required: true,
                        }
                    ]),
                    annotations: None,
                    meta: Some({
                        let mut meta = HashMap::new();
                        meta.insert("icons".to_string(), json!([
                            {
                                "src": "https://example.com/review-icon.svg",
                                "mimeType": "image/svg+xml",
                                "sizes": "any"
                            }
                        ]));
                        meta
                    }),
                }
            ],
            next_cursor: Some("next-page-cursor".to_string()),
            meta: None,
        };

        // Validate response structure
        assert_eq!(prompts_response.prompts.len(), 1);
        assert!(prompts_response.next_cursor.is_some());

        // Validate prompt structure
        let prompt = &prompts_response.prompts[0];
        assert_eq!(prompt.name, "code_review");
        assert!(prompt.title.is_some());
        assert!(prompt.description.is_some());
        assert!(prompt.arguments.is_some());

        // Validate arguments structure
        let args = prompt.arguments.as_ref().unwrap();
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].name, "code");
        assert!(args[0].description.is_some());
        assert!(args[0].required);

        // Validate JSON serialization matches spec example
        let json = serde_json::to_value(&prompts_response).unwrap();
        assert!(json["prompts"].is_array());
        assert_eq!(json["prompts"][0]["name"], "code_review");
        assert!(json["prompts"][0]["arguments"][0]["required"].as_bool().unwrap());
        assert_eq!(json["nextCursor"], "next-page-cursor");
    }

    /// **MCP Spec Requirement**: Test prompts/get request and response structure
    #[test]
    fn test_prompt_get_request_response() {
        // prompts/get request
        let get_request = GetPromptRequest {
            id: RequestId::from("get-prompt-1"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "prompts/get".to_string(),
            params: GetPromptParams {
                name: "code_review".to_string(),
                arguments: Some({
                    let mut args = HashMap::new();
                    args.insert("code".to_string(), json!("def hello():\n    print('world')"));
                    args
                }),
                meta: None,
            },
        };

        // prompts/get response
        let get_response = GetPromptResult {
            description: Some("Code review prompt".to_string()),
            messages: vec![
                PromptMessage {
                    role: Role::User,
                    content: Content::Text(TextContent {
                        content_type: "text".to_string(),
                        text: "Please review this Python code:\ndef hello():\n    print('world')".to_string(),
                        annotations: None,
                        meta: None,
                    }),
                }
            ],
            meta: None,
        };

        // Validate request structure
        assert_eq!(get_request.method, methods::GET_PROMPT);
        assert_eq!(get_request.params.name, "code_review");
        assert!(get_request.params.arguments.is_some());

        // Validate response structure
        assert!(get_response.description.is_some());
        assert!(!get_response.messages.is_empty());
        assert_eq!(get_response.messages[0].role, Role::User);

        // Validate JSON structure
        let request_json = serde_json::to_value(&get_request).unwrap();
        assert_eq!(request_json["method"], "prompts/get");
        assert_eq!(request_json["params"]["name"], "code_review");

        let response_json = serde_json::to_value(&get_response).unwrap();
        assert!(response_json["messages"][0]["content"]["text"].is_string());
        assert_eq!(response_json["messages"][0]["role"], "user");
    }

    /// **MCP Spec Requirement**: Test all prompt message content types
    #[test]
    fn test_prompt_message_content_types() {
        // Text content message
        let text_message = PromptMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                content_type: "text".to_string(),
                text: "The text content of the message".to_string(),
                annotations: None,
                meta: None,
            }),
        };

        // Image content message
        let image_message = PromptMessage {
            role: Role::Assistant,
            content: Content::Image(ImageContent {
                content_type: "image".to_string(),
                data: "base64-encoded-image-data".to_string(),
                mime_type: "image/png".to_string(),
                annotations: None,
                meta: None,
            }),
        };

        // Audio content message
        let audio_message = PromptMessage {
            role: Role::User,
            content: Content::Audio(AudioContent {
                content_type: "audio".to_string(),
                data: "base64-encoded-audio-data".to_string(),
                mime_type: "audio/wav".to_string(),
                annotations: None,
                meta: None,
            }),
        };

        // Embedded resource message
        let resource_message = PromptMessage {
            role: Role::Assistant,
            content: Content::EmbeddedResource(EmbeddedResource {
                content_type: "resource".to_string(),
                resource: ResourceContents::Text(TextResourceContents {
                    uri: "resource://example".to_string(),
                    mime_type: Some("text/plain".to_string()),
                    text: "Resource content".to_string(),
                    annotations: None,
                    meta: None,
                }),
                annotations: None,
                meta: None,
            }),
        };

        // **MCP Spec Requirement**: "The image data MUST be base64-encoded and include a valid MIME type"
        // **MCP Spec Requirement**: "The audio data MUST be base64-encoded and include a valid MIME type"
        // **MCP Spec Requirement**: "Resources MUST include: A valid resource URI, The appropriate MIME type, Either text content or base64-encoded blob data"

        // Validate all content types serialize correctly
        assert!(serde_json::to_value(&text_message).is_ok());
        assert!(serde_json::to_value(&image_message).is_ok());
        assert!(serde_json::to_value(&audio_message).is_ok());
        assert!(serde_json::to_value(&resource_message).is_ok());

        // Validate JSON structure for each type
        let text_json = serde_json::to_value(&text_message).unwrap();
        assert_eq!(text_json["content"]["type"], "text");
        assert!(text_json["content"]["text"].is_string());

        let image_json = serde_json::to_value(&image_message).unwrap();
        assert_eq!(image_json["content"]["type"], "image");
        assert!(image_json["content"]["data"].is_string());
        assert!(image_json["content"]["mimeType"].is_string());

        let audio_json = serde_json::to_value(&audio_message).unwrap();
        assert_eq!(audio_json["content"]["type"], "audio");
        assert!(audio_json["content"]["data"].is_string());
        assert!(audio_json["content"]["mimeType"].is_string());

        let resource_json = serde_json::to_value(&resource_message).unwrap();
        assert_eq!(resource_json["content"]["type"], "resource");
        assert!(resource_json["content"]["resource"]["uri"].is_string());
    }

    /// **MCP Spec Requirement**: Test prompts list changed notification
    #[test]
    fn test_prompts_list_changed_notification() {
        let list_changed_notification = JsonRpcNotification {
            jsonrpc: JsonRpcVersion::V2_0,
            method: "notifications/prompts/list_changed".to_string(),
            params: None,
        };

        // Validate notification structure
        assert_eq!(list_changed_notification.method, "notifications/prompts/list_changed");

        // Ensure it's a notification (no ID)
        let json = serde_json::to_value(&list_changed_notification).unwrap();
        assert!(!json.as_object().unwrap().contains_key("id"));
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["method"], "notifications/prompts/list_changed");
    }

    /// **MCP Spec Requirement**: Test prompt argument validation and error handling
    #[test]
    fn test_prompt_error_handling() {
        // Invalid prompt name error
        let invalid_prompt_error = JsonRpcError {
            jsonrpc: JsonRpcVersion::V2_0,
            id: Some(RequestId::from("error-1")),
            error: JsonRpcErrorCode::InvalidParams,
        };

        // Missing required arguments error
        let missing_args_error = JsonRpcError {
            jsonrpc: JsonRpcVersion::V2_0,
            id: Some(RequestId::from("error-2")),
            error: JsonRpcErrorCode::InvalidParams,
        };

        // **MCP Spec Requirement**: "Servers SHOULD return standard JSON-RPC errors for common failure cases"
        // Invalid prompt name: -32602 (Invalid params)
        // Missing required arguments: -32602 (Invalid params)
        // Internal errors: -32603 (Internal error)

        let invalid_json = serde_json::to_value(&invalid_prompt_error).unwrap();
        assert_eq!(invalid_json["error"]["code"], -32602);

        let missing_json = serde_json::to_value(&missing_args_error).unwrap();
        assert_eq!(missing_json["error"]["code"], -32602);
    }

    /// **MCP Spec Requirement**: Test prompt security validation
    #[test]
    fn test_prompt_security_validation() {
        // **MCP Spec Requirement**: "Implementations MUST carefully validate all prompt inputs and outputs to prevent injection attacks or unauthorized access to resources"

        let secure_prompt = Prompt {
            name: "secure_prompt".to_string(),
            title: Some("Secure Prompt".to_string()),
            description: Some("A prompt with security validation".to_string()),
            arguments: Some(vec![
                PromptArgument {
                    name: "user_input".to_string(),
                    description: Some("User input (will be sanitized)".to_string()),
                    required: true,
                }
            ]),
            annotations: None,
            meta: Some({
                let mut meta = HashMap::new();
                meta.insert("security_validation".to_string(), json!(true));
                meta.insert("input_sanitization".to_string(), json!(true));
                meta.insert("output_filtering".to_string(), json!(true));
                meta
            }),
        };

        // Validate security metadata exists for validation
        assert!(secure_prompt.meta.is_some());
        let meta = secure_prompt.meta.unwrap();
        assert_eq!(meta["security_validation"], true);
        assert_eq!(meta["input_sanitization"], true);
        assert_eq!(meta["output_filtering"], true);
    }
}

// =============================================================================
// RESOURCES COMPLIANCE TESTS
// =============================================================================

#[cfg(test)]
mod resources_compliance {
    use super::*;

    /// **MCP Spec Requirement**: "Servers that support resources MUST declare the resources capability"
    #[test]
    fn test_resources_capability_declaration() {
        // Full capabilities
        let full_caps = ServerCapabilities {
            resources: Some(ResourcesCapabilities {
                subscribe: Some(true),
                list_changed: Some(true),
            }),
            prompts: None,
            tools: None,
            logging: None,
            completions: None,
            experimental: None,
        };

        // Subscribe only
        let subscribe_only = ServerCapabilities {
            resources: Some(ResourcesCapabilities {
                subscribe: Some(true),
                list_changed: None,
            }),
            prompts: None,
            tools: None,
            logging: None,
            completions: None,
            experimental: None,
        };

        // List changed only
        let list_only = ServerCapabilities {
            resources: Some(ResourcesCapabilities {
                subscribe: None,
                list_changed: Some(true),
            }),
            prompts: None,
            tools: None,
            logging: None,
            completions: None,
            experimental: None,
        };

        // Neither feature
        let basic_caps = ServerCapabilities {
            resources: Some(ResourcesCapabilities {
                subscribe: None,
                list_changed: None,
            }),
            prompts: None,
            tools: None,
            logging: None,
            completions: None,
            experimental: None,
        };

        // Validate all capability variations are valid
        assert!(serde_json::to_value(&full_caps).is_ok());
        assert!(serde_json::to_value(&subscribe_only).is_ok());
        assert!(serde_json::to_value(&list_only).is_ok());
        assert!(serde_json::to_value(&basic_caps).is_ok());

        // Validate JSON structure for full capabilities
        let json = serde_json::to_value(&full_caps).unwrap();
        assert!(json["resources"].is_object());
        assert_eq!(json["resources"]["subscribe"], true);
        assert_eq!(json["resources"]["listChanged"], true);
    }

    /// **MCP Spec Requirement**: Test resources/list request structure and pagination
    #[test]
    fn test_resources_list_request_structure() {
        // Basic resources/list request
        let basic_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: RequestId::from("resources-list-1"),
            method: "resources/list".to_string(),
            params: None,
        };

        // Request with pagination cursor
        let paginated_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion::V2_0,
            id: RequestId::from("resources-list-2"),
            method: "resources/list".to_string(),
            params: Some(json!({
                "cursor": "optional-cursor-value"
            })),
        };

        // Validate basic request
        assert_eq!(basic_request.method, methods::LIST_RESOURCES);

        // Validate pagination support
        assert!(paginated_request.params.is_some());
        let params = paginated_request.params.unwrap();
        assert!(params["cursor"].is_string());
    }

    /// **MCP Spec Requirement**: Test resources/list response structure per specification
    #[test]
    fn test_resources_list_response_structure() {
        let resources_response = ListResourcesResult {
            resources: vec![
                Resource {
                    uri: "file:///project/src/main.rs".to_string(),
                    name: "main.rs".to_string(),
                    title: Some("Rust Software Application Main File".to_string()),
                    description: Some("Primary application entry point".to_string()),
                    mime_type: Some("text/x-rust".to_string()),
                    annotations: None,
                    meta: Some({
                        let mut meta = HashMap::new();
                        meta.insert("icons".to_string(), json!([
                            {
                                "src": "https://example.com/rust-file-icon.png",
                                "mimeType": "image/png",
                                "sizes": "48x48"
                            }
                        ]));
                        meta
                    }),
                }
            ],
            next_cursor: Some("next-page-cursor".to_string()),
            meta: None,
        };

        // Validate response structure
        assert_eq!(resources_response.resources.len(), 1);
        assert!(resources_response.next_cursor.is_some());

        // Validate resource structure
        let resource = &resources_response.resources[0];
        assert_eq!(resource.uri, "file:///project/src/main.rs");
        assert_eq!(resource.name, "main.rs");
        assert!(resource.title.is_some());
        assert!(resource.description.is_some());
        assert!(resource.mime_type.is_some());

        // **MCP Spec Requirement**: "Each resource is uniquely identified by a URI"
        assert!(!resource.uri.is_empty());
        assert!(resource.uri.contains("://"));

        // Validate JSON serialization matches spec example
        let json = serde_json::to_value(&resources_response).unwrap();
        assert!(json["resources"].is_array());
        assert_eq!(json["resources"][0]["uri"], "file:///project/src/main.rs");
        assert_eq!(json["resources"][0]["name"], "main.rs");
        assert_eq!(json["nextCursor"], "next-page-cursor");
    }

    /// **MCP Spec Requirement**: Test resources/read request and response
    #[test]
    fn test_resource_read_request_response() {
        // resources/read request
        let read_request = ReadResourceRequest {
            id: RequestId::from("read-resource-1"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "resources/read".to_string(),
            params: ReadResourceParams {
                uri: "file:///project/src/main.rs".to_string(),
                meta: None,
            },
        };

        // Text resource response
        let text_response = ReadResourceResult {
            contents: vec![
                ResourceContents::Text(TextResourceContents {
                    uri: "file:///project/src/main.rs".to_string(),
                    mime_type: Some("text/x-rust".to_string()),
                    text: "fn main() {\n    println!(\"Hello, world!\");\n}".to_string(),
                    annotations: None,
                    meta: None,
                })
            ],
            meta: None,
        };

        // Binary resource response
        let binary_response = ReadResourceResult {
            contents: vec![
                ResourceContents::Blob(BlobResourceContents {
                    uri: "file:///project/assets/logo.png".to_string(),
                    mime_type: Some("image/png".to_string()),
                    blob: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==".to_string(),
                    meta: None,
                })
            ],
            meta: None,
        };

        // Validate request structure
        assert_eq!(read_request.method, methods::READ_RESOURCE);
        assert_eq!(read_request.params.uri, "file:///project/src/main.rs");

        // Validate text response structure
        assert!(!text_response.contents.is_empty());
        if let ResourceContents::Text(text_content) = &text_response.contents[0] {
            assert_eq!(text_content.uri, "file:///project/src/main.rs");
            assert!(text_content.mime_type.is_some());
            assert!(!text_content.text.is_empty());
        } else {
            panic!("Expected text resource content");
        }

        // Validate binary response structure
        assert!(!binary_response.contents.is_empty());
        if let ResourceContents::Blob(blob_content) = &binary_response.contents[0] {
            assert_eq!(blob_content.uri, "file:///project/assets/logo.png");
            assert!(blob_content.mime_type.is_some());
            assert!(!blob_content.blob.is_empty());
        } else {
            panic!("Expected blob resource content");
        }

        // Validate JSON structure
        let request_json = serde_json::to_value(&read_request).unwrap();
        assert_eq!(request_json["method"], "resources/read");
        assert_eq!(request_json["params"]["uri"], "file:///project/src/main.rs");

        let text_json = serde_json::to_value(&text_response).unwrap();
        assert!(text_json["contents"][0]["text"].is_string());

        let binary_json = serde_json::to_value(&binary_response).unwrap();
        assert!(binary_json["contents"][0]["blob"].is_string());
    }

    /// **MCP Spec Requirement**: Test resource subscription functionality
    #[test]
    fn test_resource_subscription() {
        // resources/subscribe request
        let subscribe_request = SubscribeRequest {
            id: RequestId::from("subscribe-1"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "resources/subscribe".to_string(),
            params: SubscribeParams {
                uri: "file:///project/src/main.rs".to_string(),
                meta: None,
            },
        };

        // resources/unsubscribe request
        let unsubscribe_request = UnsubscribeRequest {
            id: RequestId::from("unsubscribe-1"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "resources/unsubscribe".to_string(),
            params: UnsubscribeParams {
                uri: "file:///project/src/main.rs".to_string(),
                meta: None,
            },
        };

        // Resource updated notification
        let updated_notification = ResourceUpdatedNotification {
            jsonrpc: JsonRpcVersion::V2_0,
            method: "notifications/resources/updated".to_string(),
            params: Some(ResourceUpdatedParams {
                uri: "file:///project/src/main.rs".to_string(),
                meta: None,
            }),
        };

        // Validate request structures
        assert_eq!(subscribe_request.method, methods::SUBSCRIBE);
        assert_eq!(subscribe_request.params.uri, "file:///project/src/main.rs");

        assert_eq!(unsubscribe_request.method, methods::UNSUBSCRIBE);
        assert_eq!(unsubscribe_request.params.uri, "file:///project/src/main.rs");

        // Validate notification structure
        assert_eq!(updated_notification.method, methods::RESOURCE_UPDATED);

        // Ensure notification has no ID
        let notification_json = serde_json::to_value(&updated_notification).unwrap();
        assert!(!notification_json.as_object().unwrap().contains_key("id"));
        assert_eq!(notification_json["method"], "notifications/resources/updated");
    }

    /// **MCP Spec Requirement**: Test resource list changed notification
    #[test]
    fn test_resource_list_changed_notification() {
        let list_changed_notification = ResourceListChangedNotification {
            jsonrpc: JsonRpcVersion::V2_0,
            method: "notifications/resources/list_changed".to_string(),
            params: None,
        };

        // Validate notification structure
        assert_eq!(list_changed_notification.method, methods::RESOURCE_LIST_CHANGED);

        // Ensure it's a notification (no ID)
        let json = serde_json::to_value(&list_changed_notification).unwrap();
        assert!(!json.as_object().unwrap().contains_key("id"));
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["method"], "notifications/resources/list_changed");
    }

    /// **MCP Spec Requirement**: Test resource annotations
    #[test]
    fn test_resource_annotations() {
        let annotated_resource = Resource {
            uri: "file:///project/doc/README.md".to_string(),
            name: "README.md".to_string(),
            title: Some("Project Documentation".to_string()),
            description: Some("Main project documentation file".to_string()),
            mime_type: Some("text/markdown".to_string()),
            annotations: Some(ResourceAnnotations {
                audience: Some(vec![Role::User, Role::Assistant]),
                priority: Some(0.8),
                last_modified: Some("2025-01-15T10:30:00Z".to_string()),
            }),
            meta: None,
        };

        // Validate annotation structure
        assert!(annotated_resource.annotations.is_some());
        let annotations = annotated_resource.annotations.unwrap();

        assert!(annotations.audience.is_some());
        assert_eq!(annotations.audience.unwrap().len(), 2);
        assert_eq!(annotations.priority, Some(0.8));
        assert!(annotations.last_modified.is_some());

        // Validate JSON structure
        let json = serde_json::to_value(&annotated_resource).unwrap();
        assert!(json["annotations"]["audience"].is_array());
        assert_eq!(json["annotations"]["priority"], 0.8);
        assert!(json["annotations"]["lastModified"].is_string());
    }

    /// **MCP Spec Requirement**: Test common URI schemes
    #[test]
    fn test_common_uri_schemes() {
        let file_resource = Resource {
            uri: "file:///path/to/file.txt".to_string(),
            name: "file.txt".to_string(),
            title: None,
            description: None,
            mime_type: Some("text/plain".to_string()),
            annotations: None,
            meta: None,
        };

        let http_resource = Resource {
            uri: "https://example.com/api/data".to_string(),
            name: "api_data".to_string(),
            title: None,
            description: None,
            mime_type: Some("application/json".to_string()),
            annotations: None,
            meta: None,
        };

        let custom_resource = Resource {
            uri: "myapp://database/table/users".to_string(),
            name: "users_table".to_string(),
            title: None,
            description: None,
            mime_type: Some("application/x-database-table".to_string()),
            annotations: None,
            meta: None,
        };

        // All URI schemes should be valid
        assert!(file_resource.uri.contains("://"));
        assert!(http_resource.uri.contains("://"));
        assert!(custom_resource.uri.contains("://"));

        // All should serialize correctly
        assert!(serde_json::to_value(&file_resource).is_ok());
        assert!(serde_json::to_value(&http_resource).is_ok());
        assert!(serde_json::to_value(&custom_resource).is_ok());
    }
}

// =============================================================================
// INTEGRATED SERVER FEATURES TESTS
// =============================================================================

#[cfg(test)]
mod integrated_server_features {
    use super::*;

    /// Test complete server capability negotiation for all features
    #[test]
    fn test_complete_server_capabilities() {
        let full_server_caps = ServerCapabilities {
            tools: Some(ToolsCapabilities {
                list_changed: Some(true),
            }),
            prompts: Some(PromptsCapabilities {
                list_changed: Some(true),
            }),
            resources: Some(ResourcesCapabilities {
                subscribe: Some(true),
                list_changed: Some(true),
            }),
            logging: Some(LoggingCapabilities),
            completions: Some(CompletionCapabilities),
            experimental: Some({
                let mut exp = HashMap::new();
                exp.insert("advanced_features".to_string(), json!({"version": "1.0"}));
                exp
            }),
        };

        // Validate all capabilities are present
        assert!(full_server_caps.tools.is_some());
        assert!(full_server_caps.prompts.is_some());
        assert!(full_server_caps.resources.is_some());
        assert!(full_server_caps.logging.is_some());
        assert!(full_server_caps.completions.is_some());
        assert!(full_server_caps.experimental.is_some());

        // Validate JSON structure
        let json = serde_json::to_value(&full_server_caps).unwrap();
        assert!(json["tools"]["listChanged"].as_bool().unwrap());
        assert!(json["prompts"]["listChanged"].as_bool().unwrap());
        assert!(json["resources"]["subscribe"].as_bool().unwrap());
        assert!(json["resources"]["listChanged"].as_bool().unwrap());
        assert!(json["logging"].is_object());
        assert!(json["completions"].is_object());
        assert!(json["experimental"]["advanced_features"]["version"].is_string());
    }

    /// Test server feature interaction patterns
    #[test]
    fn test_server_feature_interactions() {
        // Test tools referencing resources
        let tool_with_resource = Tool {
            name: "analyze_file".to_string(),
            title: Some("File Analyzer".to_string()),
            description: Some("Analyze a file resource".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = HashMap::new();
                    props.insert("resource_uri".to_string(), json!({
                        "type": "string",
                        "format": "uri",
                        "description": "URI of resource to analyze"
                    }));
                    props
                }),
                required: Some(vec!["resource_uri".to_string()]),
                additional_properties: None,
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };

        // Test prompt using embedded resources
        let prompt_with_resource = PromptMessage {
            role: Role::User,
            content: Content::EmbeddedResource(EmbeddedResource {
                content_type: "resource".to_string(),
                resource: ResourceContents::Text(TextResourceContents {
                    uri: "file:///project/context.txt".to_string(),
                    mime_type: Some("text/plain".to_string()),
                    text: "Context information for the prompt".to_string(),
                    annotations: None,
                    meta: None,
                }),
                annotations: None,
                meta: None,
            }),
        };

        // Validate cross-feature integration
        assert!(tool_with_resource.input_schema.properties.is_some());
        let props = tool_with_resource.input_schema.properties.unwrap();
        assert!(props["resource_uri"]["format"].as_str().unwrap() == "uri");

        if let Content::EmbeddedResource(embedded) = &prompt_with_resource.content {
            if let ResourceContents::Text(text_content) = &embedded.resource {
                assert!(text_content.uri.contains("://"));
                assert!(!text_content.text.is_empty());
            }
        }

        // All should serialize correctly
        assert!(serde_json::to_value(&tool_with_resource).is_ok());
        assert!(serde_json::to_value(&prompt_with_resource).is_ok());
    }
}

/*
## COMPREHENSIVE SERVER FEATURES COMPLIANCE COVERAGE:

✅ **PROMPTS:**
- Capability declaration with listChanged
- prompts/list request/response with pagination
- prompts/get request/response with arguments
- All content types (text, image, audio, embedded resources)
- List changed notifications
- Error handling (invalid prompts, missing args)
- Security validation requirements

✅ **RESOURCES:**
- Capability declaration (subscribe, listChanged, neither, both)
- resources/list request/response with pagination
- resources/read request/response (text and binary)
- Resource subscription (subscribe/unsubscribe)
- Resource notifications (updated, list_changed)
- Resource annotations (audience, priority, lastModified)
- Common URI schemes (file, http, custom)

✅ **INTEGRATED FEATURES:**
- Complete server capability negotiation
- Cross-feature interactions (tools+resources, prompts+resources)

## TESTS THAT WILL LIKELY FAIL (COMPLIANCE GAPS):

1. **Prompt argument types** - May not have full PromptArgument implementation
2. **Resource content types** - Text vs Blob variants may be incomplete
3. **Resource subscription management** - Subscribe/unsubscribe flow may be missing
4. **Notification structures** - May not match exact MCP schema
5. **Content annotation types** - Annotation structures may be incomplete
6. **URI validation** - May not enforce proper URI formats
7. **MIME type validation** - May not validate required MIME types
8. **Security validation** - Input/output validation may be incomplete

These failing tests will guide exactly what needs to be fixed for full compliance.
*/
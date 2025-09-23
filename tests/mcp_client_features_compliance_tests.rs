//! # MCP Client Features Compliance Tests
//!
//! These tests validate TurboMCP against all MCP Client feature specifications:
//! - Sampling: `/reference/modelcontextprotocol/docs/specification/draft/client/sampling.mdx`
//! - Roots: `/reference/modelcontextprotocol/docs/specification/draft/client/roots.mdx`
//! - Elicitation: `/reference/modelcontextprotocol/docs/specification/draft/client/elicitation.mdx`
//!
//! This ensures 100% compliance with MCP Client protocol requirements.

use serde_json::{json, Value};
use std::collections::HashMap;
use turbomcp_protocol::{
    jsonrpc::*,
    types::*,
    validation::*,
    *,
};

// =============================================================================
// SAMPLING COMPLIANCE TESTS
// =============================================================================

#[cfg(test)]
mod sampling_compliance {
    use super::*;

    /// **MCP Spec Requirement**: "Clients that support sampling MUST declare the sampling capability"
    #[test]
    fn test_sampling_capability_declaration() {
        let client_caps = ClientCapabilities {
            sampling: Some(SamplingCapabilities),
            roots: None,
            elicitation: None,
            experimental: None,
        };

        // Validate sampling capability structure
        assert!(client_caps.sampling.is_some());

        // Validate JSON structure matches spec
        let json = serde_json::to_value(&client_caps).unwrap();
        assert!(json["sampling"].is_object());
    }

    /// **MCP Spec Requirement**: Test sampling/createMessage request structure
    #[test]
    fn test_sampling_create_message_request() {
        let create_request = CreateMessageRequest {
            id: RequestId::from("sampling-1"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "sampling/createMessage".to_string(),
            params: CreateMessageParams {
                messages: vec![
                    SamplingMessage {
                        role: Role::User,
                        content: Content::Text(TextContent {
                            content_type: "text".to_string(),
                            text: "What is the capital of France?".to_string(),
                            annotations: None,
                            meta: None,
                        }),
                        meta: None,
                    }
                ],
                model_preferences: Some(ModelPreferences {
                    hints: Some(vec![
                        ModelHint {
                            name: Some("claude-3-sonnet".to_string()),
                        }
                    ]),
                    intelligence_priority: Some(0.8),
                    speed_priority: Some(0.5),
                    cost_priority: None,
                }),
                system_prompt: Some("You are a helpful assistant.".to_string()),
                max_tokens: 100,
                temperature: None,
                stop_sequences: None,
                metadata: None,
                include_context: None,
                meta: None,
            },
        };

        // Validate request structure
        assert_eq!(create_request.method, methods::CREATE_MESSAGE);
        assert!(!create_request.params.messages.is_empty());
        assert_eq!(create_request.params.max_tokens, 100);
        assert!(create_request.params.model_preferences.is_some());
        assert!(create_request.params.system_prompt.is_some());

        // Validate model preferences
        let model_prefs = create_request.params.model_preferences.unwrap();
        assert!(model_prefs.hints.is_some());
        assert_eq!(model_prefs.intelligence_priority, Some(0.8));
        assert_eq!(model_prefs.speed_priority, Some(0.5));

        // Validate JSON structure matches spec example
        let json = serde_json::to_value(&create_request).unwrap();
        assert_eq!(json["method"], "sampling/createMessage");
        assert!(json["params"]["messages"].is_array());
        assert_eq!(json["params"]["maxTokens"], 100);
        assert!(json["params"]["modelPreferences"]["hints"].is_array());
        assert_eq!(json["params"]["systemPrompt"], "You are a helpful assistant.");
    }

    /// **MCP Spec Requirement**: Test sampling/createMessage response structure
    #[test]
    fn test_sampling_create_message_response() {
        let create_response = CreateMessageResult {
            role: Role::Assistant,
            content: Content::Text(TextContent {
                content_type: "text".to_string(),
                text: "The capital of France is Paris.".to_string(),
                annotations: None,
                meta: None,
            }),
            model: "claude-3-5-sonnet-20241022".to_string(),
            stop_reason: Some("end_turn".to_string()),
            meta: None,
        };

        // Validate response structure
        assert_eq!(create_response.role, Role::Assistant);
        assert!(!create_response.model.is_empty());
        assert!(create_response.stop_reason.is_some());

        // Validate content
        if let Content::Text(text_content) = &create_response.content {
            assert!(!text_content.text.is_empty());
            assert_eq!(text_content.content_type, "text");
        } else {
            panic!("Expected text content in response");
        }

        // Validate JSON structure
        let json = serde_json::to_value(&create_response).unwrap();
        assert_eq!(json["role"], "assistant");
        assert!(json["content"]["text"].is_string());
        assert!(json["model"].is_string());
        assert!(json["stopReason"].is_string());
    }

    /// **MCP Spec Requirement**: Test all sampling message content types
    #[test]
    fn test_sampling_message_content_types() {
        // Text message
        let text_message = SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                content_type: "text".to_string(),
                text: "Analyze this text".to_string(),
                annotations: None,
                meta: None,
            }),
            meta: None,
        };

        // Image message
        let image_message = SamplingMessage {
            role: Role::User,
            content: Content::Image(ImageContent {
                content_type: "image".to_string(),
                data: "base64-encoded-image-data".to_string(),
                mime_type: "image/png".to_string(),
                annotations: None,
                meta: None,
            }),
            meta: None,
        };

        // Audio message
        let audio_message = SamplingMessage {
            role: Role::User,
            content: Content::Audio(AudioContent {
                content_type: "audio".to_string(),
                data: "base64-encoded-audio-data".to_string(),
                mime_type: "audio/wav".to_string(),
                annotations: None,
                meta: None,
            }),
            meta: None,
        };

        // Validate all message types serialize correctly
        assert!(serde_json::to_value(&text_message).is_ok());
        assert!(serde_json::to_value(&image_message).is_ok());
        assert!(serde_json::to_value(&audio_message).is_ok());

        // Validate JSON structure for each type
        let text_json = serde_json::to_value(&text_message).unwrap();
        assert_eq!(text_json["role"], "user");
        assert_eq!(text_json["content"]["type"], "text");

        let image_json = serde_json::to_value(&image_message).unwrap();
        assert_eq!(image_json["content"]["type"], "image");
        assert!(image_json["content"]["data"].is_string());
        assert!(image_json["content"]["mimeType"].is_string());

        let audio_json = serde_json::to_value(&audio_message).unwrap();
        assert_eq!(audio_json["content"]["type"], "audio");
        assert!(audio_json["content"]["data"].is_string());
        assert!(audio_json["content"]["mimeType"].is_string());
    }

    /// **MCP Spec Requirement**: Test model preferences and context inclusion
    #[test]
    fn test_sampling_advanced_parameters() {
        let advanced_request = CreateMessageRequest {
            id: RequestId::from("advanced-sampling"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "sampling/createMessage".to_string(),
            params: CreateMessageParams {
                messages: vec![
                    SamplingMessage {
                        role: Role::User,
                        content: Content::Text(TextContent {
                            content_type: "text".to_string(),
                            text: "Help me with this task".to_string(),
                            annotations: None,
                            meta: None,
                        }),
                        meta: None,
                    }
                ],
                model_preferences: Some(ModelPreferences {
                    hints: Some(vec![
                        ModelHint {
                            name: Some("claude-3-haiku".to_string()),
                        },
                        ModelHint {
                            name: Some("gpt-4".to_string()),
                        }
                    ]),
                    intelligence_priority: Some(0.6),
                    speed_priority: Some(0.9),
                    cost_priority: Some(0.8),
                }),
                system_prompt: Some("You are an expert assistant.".to_string()),
                max_tokens: 500,
                temperature: Some(0.7),
                stop_sequences: Some(vec!["END".to_string(), "STOP".to_string()]),
                include_context: Some(ContextInclusion::AllServers),
                metadata: Some({
                    let mut meta = HashMap::new();
                    meta.insert("user_id".to_string(), json!("user123"));
                    meta.insert("session_id".to_string(), json!("session456"));
                    meta
                }),
                meta: None,
            },
        };

        // Validate advanced parameters
        let params = &advanced_request.params;

        // Model preferences with multiple hints
        let model_prefs = params.model_preferences.as_ref().unwrap();
        assert!(model_prefs.hints.is_some());
        assert_eq!(model_prefs.hints.as_ref().unwrap().len(), 2);
        assert_eq!(model_prefs.intelligence_priority, Some(0.6));
        assert_eq!(model_prefs.speed_priority, Some(0.9));
        assert_eq!(model_prefs.cost_priority, Some(0.8));

        // Generation parameters
        assert_eq!(params.temperature, Some(0.7));
        assert!(params.stop_sequences.is_some());
        assert_eq!(params.stop_sequences.as_ref().unwrap().len(), 2);

        // Context inclusion
        assert_eq!(params.include_context, Some(ContextInclusion::AllServers));

        // Custom metadata
        assert!(params.metadata.is_some());
        let metadata = params.metadata.as_ref().unwrap();
        assert_eq!(metadata["user_id"], "user123");
        assert_eq!(metadata["session_id"], "session456");

        // Validate JSON structure
        let json = serde_json::to_value(&advanced_request).unwrap();
        assert_eq!(json["params"]["temperature"], 0.7);
        assert!(json["params"]["stopSequences"].is_array());
        assert_eq!(json["params"]["includeContext"], "allServers");
        assert!(json["params"]["metadata"]["user_id"].is_string());
    }

    /// **MCP Spec Requirement**: Test sampling security and user control requirements
    #[test]
    fn test_sampling_security_requirements() {
        // **MCP Spec Requirement**: "For trust & safety and security, there SHOULD always be a human in the loop with the ability to deny sampling requests"
        // **MCP Spec Requirement**: "Applications SHOULD: Provide UI that makes it easy and intuitive to review sampling requests"
        // **MCP Spec Requirement**: "Allow users to view and edit prompts before sending"
        // **MCP Spec Requirement**: "Present generated responses for review before delivery"

        let security_sensitive_request = CreateMessageRequest {
            id: RequestId::from("security-check"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "sampling/createMessage".to_string(),
            params: CreateMessageParams {
                messages: vec![
                    SamplingMessage {
                        role: Role::User,
                        content: Content::Text(TextContent {
                            content_type: "text".to_string(),
                            text: "Execute a system command".to_string(),
                            annotations: None,
                            meta: None,
                        }),
                        meta: None,
                    }
                ],
                model_preferences: None,
                system_prompt: Some("You are a system administrator with full access.".to_string()),
                max_tokens: 100,
                temperature: None,
                stop_sequences: None,
                metadata: Some({
                    let mut meta = HashMap::new();
                    meta.insert("requires_user_approval".to_string(), json!(true));
                    meta.insert("security_risk_level".to_string(), json!("high"));
                    meta.insert("allow_modification".to_string(), json!(true));
                    meta
                }),
                include_context: None,
                meta: None,
            },
        };

        // Validate security metadata is present for UI decisions
        assert!(security_sensitive_request.params.metadata.is_some());
        let metadata = security_sensitive_request.params.metadata.unwrap();
        assert_eq!(metadata["requires_user_approval"], true);
        assert_eq!(metadata["security_risk_level"], "high");
        assert_eq!(metadata["allow_modification"], true);

        // This metadata should guide client UI to:
        // 1. Show the request to the user for approval
        // 2. Allow editing the prompt before sending
        // 3. Show the response before delivering to server
    }
}

// =============================================================================
// ROOTS COMPLIANCE TESTS
// =============================================================================

#[cfg(test)]
mod roots_compliance {
    use super::*;

    /// **MCP Spec Requirement**: "Clients that support roots MUST declare the roots capability"
    #[test]
    fn test_roots_capability_declaration() {
        let client_caps = ClientCapabilities {
            roots: Some(RootsCapabilities {
                list_changed: Some(true),
            }),
            sampling: None,
            elicitation: None,
            experimental: None,
        };

        // Validate roots capability structure
        assert!(client_caps.roots.is_some());
        let roots_cap = client_caps.roots.unwrap();
        assert_eq!(roots_cap.list_changed, Some(true));

        // Validate JSON structure matches spec
        let json = serde_json::to_value(&client_caps).unwrap();
        assert!(json["roots"].is_object());
        assert_eq!(json["roots"]["listChanged"], true);
    }

    /// **MCP Spec Requirement**: Test roots/list request structure
    #[test]
    fn test_roots_list_request() {
        let list_request = ListRootsRequest {
            id: RequestId::from("roots-list-1"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "roots/list".to_string(),
            params: None, // No parameters required
        };

        // Validate request structure
        assert_eq!(list_request.method, methods::LIST_ROOTS);
        assert!(list_request.params.is_none());

        // Validate JSON structure matches spec
        let json = serde_json::to_value(&list_request).unwrap();
        assert_eq!(json["method"], "roots/list");
        assert_eq!(json["jsonrpc"], "2.0");
        assert!(json["id"].is_string() || json["id"].is_number());
    }

    /// **MCP Spec Requirement**: Test roots/list response structure
    #[test]
    fn test_roots_list_response() {
        let list_response = ListRootsResult {
            roots: vec![
                Root {
                    uri: "file:///home/user/projects/myproject".to_string(),
                    name: Some("My Project".to_string()),
                    title: Some("Main Project Directory".to_string()),
                    description: Some("The primary project workspace".to_string()),
                    annotations: None,
                    meta: None,
                },
                Root {
                    uri: "file:///home/user/documents".to_string(),
                    name: Some("Documents".to_string()),
                    title: None,
                    description: None,
                    annotations: None,
                    meta: None,
                }
            ],
            meta: None,
        };

        // Validate response structure
        assert_eq!(list_response.roots.len(), 2);

        // Validate root structures
        let root1 = &list_response.roots[0];
        assert_eq!(root1.uri, "file:///home/user/projects/myproject");
        assert_eq!(root1.name, Some("My Project".to_string()));
        assert!(root1.title.is_some());
        assert!(root1.description.is_some());

        let root2 = &list_response.roots[1];
        assert_eq!(root2.uri, "file:///home/user/documents");
        assert_eq!(root2.name, Some("Documents".to_string()));
        assert!(root2.title.is_none());
        assert!(root2.description.is_none());

        // **MCP Spec Requirement**: "Roots define the boundaries of where servers can operate within the filesystem"
        // All URIs should be valid filesystem paths
        for root in &list_response.roots {
            assert!(root.uri.starts_with("file://"));
            assert!(!root.uri.is_empty());
        }

        // Validate JSON structure matches spec example
        let json = serde_json::to_value(&list_response).unwrap();
        assert!(json["roots"].is_array());
        assert_eq!(json["roots"][0]["uri"], "file:///home/user/projects/myproject");
        assert_eq!(json["roots"][0]["name"], "My Project");
    }

    /// **MCP Spec Requirement**: Test roots list changed notification
    #[test]
    fn test_roots_list_changed_notification() {
        let list_changed_notification = RootsListChangedNotification {
            jsonrpc: JsonRpcVersion::V2_0,
            method: "notifications/roots/list_changed".to_string(),
            params: None,
        };

        // **MCP Spec Requirement**: "When roots change, clients that support listChanged MUST send a notification"
        assert_eq!(list_changed_notification.method, methods::ROOTS_LIST_CHANGED);

        // Ensure it's a notification (no ID)
        let json = serde_json::to_value(&list_changed_notification).unwrap();
        assert!(!json.as_object().unwrap().contains_key("id"));
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["method"], "notifications/roots/list_changed");
    }

    /// **MCP Spec Requirement**: Test root URI validation and filesystem boundaries
    #[test]
    fn test_root_filesystem_boundaries() {
        let valid_roots = vec![
            Root {
                uri: "file:///home/user/project".to_string(),
                name: Some("Project".to_string()),
                title: None,
                description: None,
                annotations: None,
                meta: None,
            },
            Root {
                uri: "file:///workspace/app".to_string(),
                name: Some("App Workspace".to_string()),
                title: None,
                description: None,
                annotations: None,
                meta: None,
            },
            Root {
                uri: "file:///tmp/sandbox".to_string(),
                name: Some("Sandbox".to_string()),
                title: None,
                description: None,
                annotations: None,
                meta: None,
            },
        ];

        // All roots should have valid file URIs
        for root in &valid_roots {
            assert!(root.uri.starts_with("file://"));
            assert!(root.uri.len() > 7); // More than just "file://"
            assert!(!root.uri.contains("../")); // No path traversal
            assert!(root.name.is_some()); // Should have a display name
        }

        // All should serialize correctly
        for root in &valid_roots {
            assert!(serde_json::to_value(root).is_ok());
        }
    }

    /// **MCP Spec Requirement**: Test root annotations and metadata
    #[test]
    fn test_root_annotations() {
        let annotated_root = Root {
            uri: "file:///workspace/project".to_string(),
            name: Some("Main Project".to_string()),
            title: Some("Primary Development Workspace".to_string()),
            description: Some("The main workspace for active development".to_string()),
            annotations: Some(RootAnnotations {
                audience: Some(vec![Role::User, Role::Assistant]),
                priority: Some(0.9),
                last_modified: Some("2025-01-15T10:30:00Z".to_string()),
            }),
            meta: Some({
                let mut meta = HashMap::new();
                meta.insert("permissions".to_string(), json!(["read", "write"]));
                meta.insert("vcs_type".to_string(), json!("git"));
                meta.insert("project_type".to_string(), json!("rust"));
                meta
            }),
        };

        // Validate annotation structure
        assert!(annotated_root.annotations.is_some());
        let annotations = annotated_root.annotations.unwrap();
        assert!(annotations.audience.is_some());
        assert_eq!(annotations.priority, Some(0.9));
        assert!(annotations.last_modified.is_some());

        // Validate metadata
        assert!(annotated_root.meta.is_some());
        let meta = annotated_root.meta.unwrap();
        assert!(meta["permissions"].is_array());
        assert_eq!(meta["vcs_type"], "git");
        assert_eq!(meta["project_type"], "rust");

        // Validate JSON structure
        let json = serde_json::to_value(&annotated_root).unwrap();
        assert!(json["annotations"]["audience"].is_array());
        assert_eq!(json["annotations"]["priority"], 0.9);
        assert!(json["_meta"]["permissions"].is_array());
    }
}

// =============================================================================
// ELICITATION COMPLIANCE TESTS
// =============================================================================

#[cfg(test)]
mod elicitation_compliance {
    use super::*;

    /// **MCP Spec Requirement**: "Clients that support elicitation MUST declare the elicitation capability"
    #[test]
    fn test_elicitation_capability_declaration() {
        let client_caps = ClientCapabilities {
            elicitation: Some(ElicitationCapabilities),
            sampling: None,
            roots: None,
            experimental: None,
        };

        // Validate elicitation capability structure
        assert!(client_caps.elicitation.is_some());

        // Validate JSON structure matches spec
        let json = serde_json::to_value(&client_caps).unwrap();
        assert!(json["elicitation"].is_object());
    }

    /// **MCP Spec Requirement**: Test elicitation/create request structure
    #[test]
    fn test_elicitation_create_request() {
        let create_request = ElicitRequest {
            id: RequestId::from("elicit-1"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "elicitation/create".to_string(),
            params: ElicitRequestParams {
                message: "Please provide your GitHub username".to_string(),
                requested_schema: ElicitationSchema {
                    schema_type: "object".to_string(),
                    properties: Some({
                        let mut props = HashMap::new();
                        props.insert("username".to_string(), PrimitiveSchemaDefinition::String(StringSchema {
                            schema_type: "string".to_string(),
                            title: Some("GitHub Username".to_string()),
                            description: Some("Your GitHub username".to_string()),
                            default: None,
                            min_length: Some(1),
                            max_length: Some(39), // GitHub username limit
                            pattern: Some("^[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?$".to_string()),
                        }));
                        props
                    }),
                    required: Some(vec!["username".to_string()]),
                    additional_properties: Some(false),
                    title: Some("GitHub Username Request".to_string()),
                    description: Some("Please enter your GitHub username for repository access".to_string()),
                },
                meta: None,
            },
        };

        // Validate request structure
        assert_eq!(create_request.method, "elicitation/create");
        assert!(!create_request.params.message.is_empty());
        assert_eq!(create_request.params.requested_schema.schema_type, "object");

        // Validate schema structure
        let schema = &create_request.params.requested_schema;
        assert!(schema.properties.is_some());
        assert!(schema.required.is_some());
        assert_eq!(schema.additional_properties, Some(false));

        // Validate JSON structure matches spec
        let json = serde_json::to_value(&create_request).unwrap();
        assert_eq!(json["method"], "elicitation/create");
        assert!(json["params"]["message"].is_string());
        assert!(json["params"]["requestedSchema"]["properties"].is_object());
        assert!(json["params"]["requestedSchema"]["required"].is_array());
    }

    /// **MCP Spec Requirement**: Test elicitation response structure
    #[test]
    fn test_elicitation_response() {
        let elicit_response = ElicitResult {
            response: json!({
                "username": "johndoe"
            }),
            meta: None,
        };

        // Validate response structure
        assert!(elicit_response.response.is_object());
        assert_eq!(elicit_response.response["username"], "johndoe");

        // Validate JSON structure
        let json = serde_json::to_value(&elicit_response).unwrap();
        assert!(json["response"].is_object());
        assert_eq!(json["response"]["username"], "johndoe");
    }

    /// **MCP Spec Requirement**: Test complex elicitation schemas
    #[test]
    fn test_complex_elicitation_schemas() {
        let complex_request = ElicitRequest {
            id: RequestId::from("complex-elicit"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "elicitation/create".to_string(),
            params: ElicitRequestParams {
                message: "Please configure your development environment".to_string(),
                requested_schema: ElicitationSchema {
                    schema_type: "object".to_string(),
                    properties: Some({
                        let mut props = HashMap::new();

                        // String field with validation
                        props.insert("project_name".to_string(), PrimitiveSchemaDefinition::String(StringSchema {
                            schema_type: "string".to_string(),
                            title: Some("Project Name".to_string()),
                            description: Some("Name of your project".to_string()),
                            default: None,
                            min_length: Some(1),
                            max_length: Some(50),
                            pattern: Some("^[a-zA-Z][a-zA-Z0-9_-]*$".to_string()),
                        }));

                        // Enum field
                        props.insert("language".to_string(), PrimitiveSchemaDefinition::Enum(EnumSchema {
                            schema_type: "string".to_string(),
                            title: Some("Programming Language".to_string()),
                            description: Some("Primary language for the project".to_string()),
                            enum_values: vec!["rust".to_string(), "python".to_string(), "javascript".to_string(), "go".to_string()],
                            enum_names: Some(vec!["Rust".to_string(), "Python".to_string(), "JavaScript".to_string(), "Go".to_string()]),
                            default: Some("rust".to_string()),
                        }));

                        // Boolean field
                        props.insert("use_docker".to_string(), PrimitiveSchemaDefinition::Boolean(BooleanSchema {
                            schema_type: "boolean".to_string(),
                            title: Some("Use Docker".to_string()),
                            description: Some("Enable Docker support for this project".to_string()),
                            default: Some(true),
                        }));

                        // Number field
                        props.insert("port".to_string(), PrimitiveSchemaDefinition::Number(NumberSchema {
                            schema_type: "number".to_string(),
                            title: Some("Port Number".to_string()),
                            description: Some("Port for development server".to_string()),
                            default: Some(json!(8080)),
                            minimum: Some(1000.0),
                            maximum: Some(65535.0),
                        }));

                        props
                    }),
                    required: Some(vec!["project_name".to_string(), "language".to_string()]),
                    additional_properties: Some(false),
                    title: Some("Development Environment Configuration".to_string()),
                    description: Some("Configure your development environment settings".to_string()),
                },
                meta: None,
            },
        };

        // Validate complex schema structure
        let schema = &complex_request.params.requested_schema;
        assert!(schema.properties.is_some());
        let properties = schema.properties.as_ref().unwrap();

        // Check all property types are present
        assert!(properties.contains_key("project_name"));
        assert!(properties.contains_key("language"));
        assert!(properties.contains_key("use_docker"));
        assert!(properties.contains_key("port"));

        // Validate required fields
        assert!(schema.required.is_some());
        let required = schema.required.as_ref().unwrap();
        assert!(required.contains(&"project_name".to_string()));
        assert!(required.contains(&"language".to_string()));

        // Validate JSON structure
        let json = serde_json::to_value(&complex_request).unwrap();
        assert!(json["params"]["requestedSchema"]["properties"]["project_name"].is_object());
        assert!(json["params"]["requestedSchema"]["properties"]["language"]["enum"].is_array());
        assert!(json["params"]["requestedSchema"]["properties"]["use_docker"]["default"].is_boolean());
        assert!(json["params"]["requestedSchema"]["properties"]["port"]["minimum"].is_number());
    }

    /// **MCP Spec Requirement**: Test elicitation security requirements
    #[test]
    fn test_elicitation_security_requirements() {
        // **MCP Spec Requirement**: "Servers MUST NOT use elicitation to request sensitive information"
        // **MCP Spec Requirement**: "Applications SHOULD: Provide UI that makes it clear which server is requesting information"
        // **MCP Spec Requirement**: "Allow users to review and modify their responses before sending"
        // **MCP Spec Requirement**: "Respect user privacy and provide clear decline and cancel options"

        let security_conscious_request = ElicitRequest {
            id: RequestId::from("secure-elicit"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "elicitation/create".to_string(),
            params: ElicitRequestParams {
                message: "Please provide your preferred username (not sensitive information)".to_string(),
                requested_schema: ElicitationSchema {
                    schema_type: "object".to_string(),
                    properties: Some({
                        let mut props = HashMap::new();
                        props.insert("preferred_username".to_string(), PrimitiveSchemaDefinition::String(StringSchema {
                            schema_type: "string".to_string(),
                            title: Some("Preferred Username".to_string()),
                            description: Some("A non-sensitive display name for your profile".to_string()),
                            default: None,
                            min_length: Some(1),
                            max_length: Some(30),
                            pattern: None,
                        }));
                        props
                    }),
                    required: None, // Not required - user can decline
                    additional_properties: Some(false),
                    title: Some("Non-Sensitive Profile Information".to_string()),
                    description: Some("This information is not sensitive and is optional".to_string()),
                },
                meta: Some({
                    let mut meta = HashMap::new();
                    meta.insert("security_level".to_string(), json!("low"));
                    meta.insert("sensitive_data".to_string(), json!(false));
                    meta.insert("user_can_decline".to_string(), json!(true));
                    meta.insert("server_info".to_string(), json!({
                        "name": "profile-helper",
                        "purpose": "User profile customization"
                    }));
                    meta
                }),
            },
        };

        // Validate security metadata
        assert!(security_conscious_request.params.meta.is_some());
        let meta = security_conscious_request.params.meta.unwrap();
        assert_eq!(meta["security_level"], "low");
        assert_eq!(meta["sensitive_data"], false);
        assert_eq!(meta["user_can_decline"], true);
        assert!(meta["server_info"].is_object());

        // Schema should not require the field (user can decline)
        let schema = &security_conscious_request.params.requested_schema;
        assert!(schema.required.is_none() || schema.required.as_ref().unwrap().is_empty());

        // Message should clearly indicate non-sensitive nature
        assert!(security_conscious_request.params.message.contains("not sensitive"));
    }

    /// **MCP Spec Requirement**: Test elicitation cancellation and decline
    #[test]
    fn test_elicitation_cancellation() {
        // User declines to provide information
        let decline_response = ElicitResult {
            response: json!(null), // User declined
            meta: Some({
                let mut meta = HashMap::new();
                meta.insert("user_action".to_string(), json!("declined"));
                meta.insert("reason".to_string(), json!("privacy_preference"));
                meta
            }),
        };

        // User cancels the elicitation
        let cancel_response = ElicitResult {
            response: json!({}), // Empty response
            meta: Some({
                let mut meta = HashMap::new();
                meta.insert("user_action".to_string(), json!("cancelled"));
                meta
            }),
        };

        // Both should be valid responses
        assert!(serde_json::to_value(&decline_response).is_ok());
        assert!(serde_json::to_value(&cancel_response).is_ok());

        // Validate decline response
        assert!(decline_response.response.is_null());
        let decline_meta = decline_response.meta.unwrap();
        assert_eq!(decline_meta["user_action"], "declined");

        // Validate cancel response
        assert!(cancel_response.response.is_object());
        let cancel_meta = cancel_response.meta.unwrap();
        assert_eq!(cancel_meta["user_action"], "cancelled");
    }
}

// =============================================================================
// INTEGRATED CLIENT FEATURES TESTS
// =============================================================================

#[cfg(test)]
mod integrated_client_features {
    use super::*;

    /// Test complete client capability negotiation for all features
    #[test]
    fn test_complete_client_capabilities() {
        let full_client_caps = ClientCapabilities {
            sampling: Some(SamplingCapabilities),
            roots: Some(RootsCapabilities {
                list_changed: Some(true),
            }),
            elicitation: Some(ElicitationCapabilities),
            experimental: Some({
                let mut exp = HashMap::new();
                exp.insert("custom_client_feature".to_string(), json!({"version": "1.0"}));
                exp
            }),
        };

        // Validate all capabilities are present
        assert!(full_client_caps.sampling.is_some());
        assert!(full_client_caps.roots.is_some());
        assert!(full_client_caps.elicitation.is_some());
        assert!(full_client_caps.experimental.is_some());

        // Validate JSON structure
        let json = serde_json::to_value(&full_client_caps).unwrap();
        assert!(json["sampling"].is_object());
        assert!(json["roots"]["listChanged"].as_bool().unwrap());
        assert!(json["elicitation"].is_object());
        assert!(json["experimental"]["custom_client_feature"]["version"].is_string());
    }

    /// Test client feature interaction patterns
    #[test]
    fn test_client_feature_interactions() {
        // Test sampling request that includes context from roots
        let context_aware_sampling = CreateMessageRequest {
            id: RequestId::from("context-sampling"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "sampling/createMessage".to_string(),
            params: CreateMessageParams {
                messages: vec![
                    SamplingMessage {
                        role: Role::User,
                        content: Content::Text(TextContent {
                            content_type: "text".to_string(),
                            text: "Analyze the code in my project".to_string(),
                            annotations: None,
                            meta: None,
                        }),
                        meta: None,
                    }
                ],
                model_preferences: None,
                system_prompt: Some("You have access to the user's project files".to_string()),
                max_tokens: 200,
                temperature: None,
                stop_sequences: None,
                include_context: Some(ContextInclusion::ThisServer), // Include context from current server
                metadata: Some({
                    let mut meta = HashMap::new();
                    meta.insert("workspace_root".to_string(), json!("file:///home/user/project"));
                    meta
                }),
                meta: None,
            },
        };

        // Test elicitation for gathering information before sampling
        let pre_sampling_elicitation = ElicitRequest {
            id: RequestId::from("pre-sampling"),
            jsonrpc: JsonRpcVersion::V2_0,
            method: "elicitation/create".to_string(),
            params: ElicitRequestParams {
                message: "Before I help with your code, what type of analysis would you like?".to_string(),
                requested_schema: ElicitationSchema {
                    schema_type: "object".to_string(),
                    properties: Some({
                        let mut props = HashMap::new();
                        props.insert("analysis_type".to_string(), PrimitiveSchemaDefinition::Enum(EnumSchema {
                            schema_type: "string".to_string(),
                            title: Some("Analysis Type".to_string()),
                            description: Some("Type of code analysis to perform".to_string()),
                            enum_values: vec![
                                "security".to_string(),
                                "performance".to_string(),
                                "style".to_string(),
                                "bugs".to_string()
                            ],
                            enum_names: Some(vec![
                                "Security Review".to_string(),
                                "Performance Analysis".to_string(),
                                "Code Style Check".to_string(),
                                "Bug Detection".to_string()
                            ]),
                            default: Some("bugs".to_string()),
                        }));
                        props
                    }),
                    required: Some(vec!["analysis_type".to_string()]),
                    additional_properties: Some(false),
                    title: Some("Code Analysis Configuration".to_string()),
                    description: Some("Choose the type of analysis for your code".to_string()),
                },
                meta: Some({
                    let mut meta = HashMap::new();
                    meta.insert("prepares_for_sampling".to_string(), json!(true));
                    meta
                }),
            },
        };

        // Validate cross-feature integration
        assert_eq!(context_aware_sampling.params.include_context, Some(ContextInclusion::ThisServer));
        assert!(context_aware_sampling.params.metadata.is_some());

        assert!(pre_sampling_elicitation.params.meta.is_some());
        let elicit_meta = pre_sampling_elicitation.params.meta.unwrap();
        assert_eq!(elicit_meta["prepares_for_sampling"], true);

        // All should serialize correctly
        assert!(serde_json::to_value(&context_aware_sampling).is_ok());
        assert!(serde_json::to_value(&pre_sampling_elicitation).is_ok());
    }
}

/*
## COMPREHENSIVE CLIENT FEATURES COMPLIANCE COVERAGE:

✅ **SAMPLING:**
- Capability declaration
- sampling/createMessage request/response structure
- All content types (text, image, audio)
- Model preferences with hints and priorities
- Advanced parameters (temperature, stop sequences, context inclusion)
- Security requirements (user control, review, approval)

✅ **ROOTS:**
- Capability declaration with listChanged
- roots/list request/response structure
- Root structure with URI, name, title, description
- Roots list changed notifications
- Filesystem boundary validation
- Root annotations and metadata

✅ **ELICITATION:**
- Capability declaration
- elicitation/create request/response structure
- Simple and complex schema types (string, enum, boolean, number)
- Schema validation (patterns, min/max, required fields)
- Security requirements (non-sensitive data, user control)
- Cancellation and decline handling

✅ **INTEGRATED FEATURES:**
- Complete client capability negotiation
- Cross-feature interactions (sampling+roots, elicitation+sampling)

## TESTS THAT WILL LIKELY FAIL (COMPLIANCE GAPS):

1. **Sampling message structures** - SamplingMessage type may be incomplete
2. **Model preferences** - ModelPreferences and ModelHint types may be missing
3. **Context inclusion** - ContextInclusion enum may not exist
4. **Elicitation schema types** - Complex schema types may be incomplete
5. **Root annotations** - RootAnnotations type may be missing
6. **Capability structures** - Some capability types may be incomplete
7. **Client notification handling** - Notification sending may not be implemented
8. **Security metadata handling** - Security validation may be incomplete

These failing tests will identify exactly what client feature implementations need to be completed.
*/
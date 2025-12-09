//! Comprehensive tests
use super::*;

//! Comprehensive tests for protocol types

use serde_json::json;
use std::collections::HashMap;
use crate::types::*;

// ============================================================================
// Type Aliases Tests
// ============================================================================

#[test]
fn test_type_aliases() {
    let _protocol_version: ProtocolVersion = "1.0.0".to_string();
    let _uri: Uri = "https://example.com".to_string();
    let _mime_type: MimeType = "text/plain".to_string();
    let _base64: Base64String = "SGVsbG8gV29ybGQ=".to_string();
    let _cursor: Cursor = "next_page".to_string();
}

// ============================================================================
// Error Codes Tests
// ============================================================================

#[test]
fn test_error_codes() {
    assert_eq!(error_codes::PARSE_ERROR, -32700);
    assert_eq!(error_codes::INVALID_REQUEST, -32600);
    assert_eq!(error_codes::METHOD_NOT_FOUND, -32601);
    assert_eq!(error_codes::INVALID_PARAMS, -32602);
    assert_eq!(error_codes::INTERNAL_ERROR, -32603);
}

// ============================================================================
// JsonRpcError Tests
// ============================================================================

#[test]
fn test_jsonrpc_error_new() {
    let error = JsonRpcError::new(404, "Not found".to_string());
    assert_eq!(error.code, 404);
    assert_eq!(error.message, "Not found");
    assert!(error.data.is_none());
}

#[test]
fn test_jsonrpc_error_with_data() {
    let data = json!({"details": "More info"});
    let error = JsonRpcError::with_data(500, "Server error".to_string(), data.clone());
    assert_eq!(error.code, 500);
    assert_eq!(error.message, "Server error");
    assert_eq!(error.data, Some(data));
}

#[test]
fn test_jsonrpc_error_parse_error() {
    let error = JsonRpcError::parse_error();
    assert_eq!(error.code, error_codes::PARSE_ERROR);
    assert_eq!(error.message, "Parse error");
}

#[test]
fn test_jsonrpc_error_invalid_request() {
    let error = JsonRpcError::invalid_request();
    assert_eq!(error.code, error_codes::INVALID_REQUEST);
    assert_eq!(error.message, "Invalid Request");
}

#[test]
fn test_jsonrpc_error_method_not_found() {
    let error = JsonRpcError::method_not_found("test_method");
    assert_eq!(error.code, error_codes::METHOD_NOT_FOUND);
    assert_eq!(error.message, "Method not found: test_method");
}

#[test]
fn test_jsonrpc_error_invalid_params() {
    let error = JsonRpcError::invalid_params("missing required field");
    assert_eq!(error.code, error_codes::INVALID_PARAMS);
    assert_eq!(error.message, "Invalid params: missing required field");
}

#[test]
fn test_jsonrpc_error_internal_error() {
    let error = JsonRpcError::internal_error("database connection failed");
    assert_eq!(error.code, error_codes::INTERNAL_ERROR);
    assert_eq!(error.message, "Internal error: database connection failed");
}

#[test]
fn test_jsonrpc_error_serialization() {
    let error = JsonRpcError::new(400, "Bad Request".to_string());
    let json = serde_json::to_string(&error).unwrap();
    let deserialized: JsonRpcError = serde_json::from_str(&json).unwrap();

    assert_eq!(error.code, deserialized.code);
    assert_eq!(error.message, deserialized.message);
    assert_eq!(error.data, deserialized.data);
}

#[test]
fn test_jsonrpc_error_with_data_serialization() {
    let data = json!({"field": "value", "number": 42});
    let error = JsonRpcError::with_data(422, "Validation error".to_string(), data.clone());

    let json = serde_json::to_string(&error).unwrap();
    let deserialized: JsonRpcError = serde_json::from_str(&json).unwrap();

    assert_eq!(error.code, deserialized.code);
    assert_eq!(error.message, deserialized.message);
    assert_eq!(error.data, deserialized.data);
    assert_eq!(deserialized.data, Some(data));
}

#[test]
fn test_jsonrpc_error_clone() {
    let original = JsonRpcError::new(500, "Error".to_string());
    let cloned = original.clone();
    assert_eq!(original.code, cloned.code);
    assert_eq!(original.message, cloned.message);
}

#[test]
fn test_jsonrpc_error_debug() {
    let error = JsonRpcError::new(404, "Not found".to_string());
    let debug = format!("{error:?}");
    assert!(debug.contains("JsonRpcError"));
    assert!(debug.contains("404"));
    assert!(debug.contains("Not found"));
}

#[test]
fn test_jsonrpc_error_equality() {
    let error1 = JsonRpcError::new(400, "Bad Request".to_string());
    let error2 = JsonRpcError::new(400, "Bad Request".to_string());
    let error3 = JsonRpcError::new(404, "Not Found".to_string());

    assert_eq!(error1, error2);
    assert_ne!(error1, error3);
}

// ============================================================================
// BaseMetadata Tests
// ============================================================================

#[test]
fn test_base_metadata() {
    let metadata = BaseMetadata {
        name: "test_name".to_string(),
        title: Some("Test Title".to_string()),
    };

    assert_eq!(metadata.name, "test_name");
    assert_eq!(metadata.title, Some("Test Title".to_string()));
}

#[test]
fn test_base_metadata_no_title() {
    let metadata = BaseMetadata {
        name: "test_name".to_string(),
        title: None,
    };

    assert_eq!(metadata.name, "test_name");
    assert_eq!(metadata.title, None);
}

#[test]
fn test_base_metadata_serialization() {
    let metadata = BaseMetadata {
        name: "test".to_string(),
        title: Some("Test".to_string()),
    };

    let json = serde_json::to_string(&metadata).unwrap();
    let deserialized: BaseMetadata = serde_json::from_str(&json).unwrap();

    assert_eq!(metadata.name, deserialized.name);
    assert_eq!(metadata.title, deserialized.title);
}

// ============================================================================
// Implementation Tests
// ============================================================================

#[test]
fn test_implementation() {
    let impl_info = Implementation {
        name: "test-server".to_string(),
        title: Some("Test Server".to_string()),
        version: "1.0.0".to_string(),
        #[cfg(feature = "mcp-draft")]
        description: None,
        #[cfg(feature = "mcp-icons")]
        icons: None,
    };

    assert_eq!(impl_info.name, "test-server");
    assert_eq!(impl_info.title, Some("Test Server".to_string()));
    assert_eq!(impl_info.version, "1.0.0");
}

#[test]
fn test_implementation_no_title() {
    let impl_info = Implementation {
        name: "minimal-server".to_string(),
        title: None,
        version: "0.1.0".to_string(),
        #[cfg(feature = "mcp-draft")]
        description: None,
        #[cfg(feature = "mcp-icons")]
        icons: None,
    };

    assert_eq!(impl_info.name, "minimal-server");
    assert_eq!(impl_info.title, None);
    assert_eq!(impl_info.version, "0.1.0");
}

#[test]
fn test_implementation_serialization() {
    let impl_info = Implementation {
        name: "server".to_string(),
        title: Some("Server".to_string()),
        version: "2.0.0".to_string(),
        #[cfg(feature = "mcp-draft")]
        description: None,
        #[cfg(feature = "mcp-icons")]
        icons: None,
    };

    let json = serde_json::to_string(&impl_info).unwrap();
    let deserialized: Implementation = serde_json::from_str(&json).unwrap();

    assert_eq!(impl_info.name, deserialized.name);
    assert_eq!(impl_info.title, deserialized.title);
    assert_eq!(impl_info.version, deserialized.version);
}

// ============================================================================
// Annotations Tests
// ============================================================================

#[test]
fn test_annotations_default() {
    let annotations = Annotations::default();
    assert!(annotations.audience.is_none());
    assert!(annotations.priority.is_none());
    assert!(annotations.custom.is_empty());
}

#[test]
fn test_annotations_with_values() {
    let mut custom = HashMap::new();
    custom.insert("key1".to_string(), json!("value1"));
    custom.insert("key2".to_string(), json!(42));

    let annotations = Annotations {
        audience: Some(vec!["developers".to_string(), "users".to_string()]),
        priority: Some(1.5),
        custom,
        ..Default::default()
    };

    assert_eq!(
        annotations.audience,
        Some(vec!["developers".to_string(), "users".to_string()])
    );
    assert_eq!(annotations.priority, Some(1.5));
    assert_eq!(annotations.custom.len(), 2);
}

#[test]
fn test_annotations_serialization() {
    let mut custom = HashMap::new();
    custom.insert("test".to_string(), json!("value"));

    let annotations = Annotations {
        audience: Some(vec!["test".to_string()]),
        priority: Some(2.0),
        custom,
        ..Default::default()
    };

    let json = serde_json::to_string(&annotations).unwrap();
    let deserialized: Annotations = serde_json::from_str(&json).unwrap();

    assert_eq!(annotations.audience, deserialized.audience);
    assert_eq!(annotations.priority, deserialized.priority);
    assert_eq!(annotations.custom.len(), deserialized.custom.len());
}

// ============================================================================
// Role Tests
// ============================================================================

#[test]
fn test_role_variants() {
    let user_role = Role::User;
    let assistant_role = Role::Assistant;

    assert!(matches!(user_role, Role::User));
    assert!(matches!(assistant_role, Role::Assistant));
}

#[test]
fn test_role_serialization() {
    let user_json = serde_json::to_string(&Role::User).unwrap();
    let assistant_json = serde_json::to_string(&Role::Assistant).unwrap();

    assert_eq!(user_json, "\"user\"");
    assert_eq!(assistant_json, "\"assistant\"");

    let user_deser: Role = serde_json::from_str("\"user\"").unwrap();
    let assistant_deser: Role = serde_json::from_str("\"assistant\"").unwrap();

    assert!(matches!(user_deser, Role::User));
    assert!(matches!(assistant_deser, Role::Assistant));
}

#[test]
fn test_role_clone() {
    let original = Role::User;
    let cloned = original;
    assert!(matches!(cloned, Role::User));
}

// ============================================================================
// LogLevel Tests
// ============================================================================

#[test]
fn test_log_level_variants() {
    let levels = [
        LogLevel::Debug,
        LogLevel::Info,
        LogLevel::Notice,
        LogLevel::Warning,
        LogLevel::Error,
        LogLevel::Critical,
        LogLevel::Alert,
        LogLevel::Emergency,
    ];

    for level in levels {
        let json = serde_json::to_string(&level).unwrap();
        let deserialized: LogLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{level:?}"), format!("{:?}", deserialized));
    }
}

#[test]
fn test_log_level_serialization() {
    assert_eq!(
        serde_json::to_string(&LogLevel::Debug).unwrap(),
        "\"debug\""
    );
    assert_eq!(serde_json::to_string(&LogLevel::Info).unwrap(), "\"info\"");
    assert_eq!(
        serde_json::to_string(&LogLevel::Error).unwrap(),
        "\"error\""
    );
}

#[test]
fn test_log_level_copy() {
    let original = LogLevel::Warning;
    let copied = original;
    assert!(matches!(copied, LogLevel::Warning));
    assert!(matches!(original, LogLevel::Warning)); // Still accessible
}

// ============================================================================
// Content Types Tests
// ============================================================================

#[test]
fn test_text_content() {
    let text_content = TextContent {
        text: "Hello, World!".to_string(),
        annotations: None,
        meta: None,
    };

    assert_eq!(text_content.text, "Hello, World!");
    assert!(text_content.annotations.is_none());
    assert!(text_content.meta.is_none());
}

#[test]
fn test_text_content_with_annotations() {
    let annotations = Annotations::default();
    let mut meta = HashMap::new();
    meta.insert("source".to_string(), json!("test"));

    let text_content = TextContent {
        text: "Content with meta".to_string(),
        annotations: Some(annotations),
        meta: Some(meta),
    };

    assert_eq!(text_content.text, "Content with meta");
    assert!(text_content.annotations.is_some());
    assert!(text_content.meta.is_some());
}

#[test]
fn test_image_content() {
    let image_content = ImageContent {
        data: "base64encodedimage".to_string(),
        mime_type: "image/png".to_string(),
        annotations: None,
        meta: None,
    };

    assert_eq!(image_content.data, "base64encodedimage");
    assert_eq!(image_content.mime_type, "image/png");
}

#[test]
fn test_audio_content() {
    let audio_content = AudioContent {
        data: "base64encodedaudio".to_string(),
        mime_type: "audio/wav".to_string(),
        annotations: None,
        meta: None,
    };

    assert_eq!(audio_content.data, "base64encodedaudio");
    assert_eq!(audio_content.mime_type, "audio/wav");
}

#[test]
fn test_content_block_variants() {
    let text = ContentBlock::Text(TextContent {
        text: "Hello".to_string(),
        annotations: None,
        meta: None,
    });

    let image = ContentBlock::Image(ImageContent {
        data: "image_data".to_string(),
        mime_type: "image/jpeg".to_string(),
        annotations: None,
        meta: None,
    });

    match text {
        ContentBlock::Text(content) => assert_eq!(content.text, "Hello"),
        _ => panic!("Expected text content"),
    }

    match image {
        ContentBlock::Image(content) => assert_eq!(content.mime_type, "image/jpeg"),
        _ => panic!("Expected image content"),
    }
}

#[test]
fn test_content_block_serialization() {
    let text_content = ContentBlock::Text(TextContent {
        text: "Test text".to_string(),
        annotations: None,
        meta: None,
    });

    let json = serde_json::to_string(&text_content).unwrap();
    let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();

    match deserialized {
        ContentBlock::Text(content) => assert_eq!(content.text, "Test text"),
        _ => panic!("Expected text content"),
    }
}

#[test]
fn test_content_alias() {
    let text_content: Content = ContentBlock::Text(TextContent {
        text: "Alias test".to_string(),
        annotations: None,
        meta: None,
    });

    match text_content {
        ContentBlock::Text(content) => assert_eq!(content.text, "Alias test"),
        _ => panic!("Expected text content"),
    }
}

// ============================================================================
// Capability Tests
// ============================================================================

#[test]
fn test_client_capabilities_default() {
    let capabilities = ClientCapabilities::default();
    assert!(capabilities.experimental.is_none());
    assert!(capabilities.roots.is_none());
    assert!(capabilities.sampling.is_none());
    assert!(capabilities.elicitation.is_none());
}

#[test]
fn test_client_capabilities_with_values() {
    let mut experimental = HashMap::new();
    experimental.insert("feature1".to_string(), json!(true));

    let capabilities = ClientCapabilities {
        experimental: Some(experimental),
        roots: Some(RootsCapabilities {
            list_changed: Some(true),
        }),
        sampling: Some(SamplingCapabilities),
        elicitation: Some(ElicitationCapabilities::default()),
    };

    assert!(capabilities.experimental.is_some());
    assert!(capabilities.roots.is_some());
    assert!(capabilities.sampling.is_some());
    assert!(capabilities.elicitation.is_some());
}

#[test]
fn test_server_capabilities_default() {
    let capabilities = ServerCapabilities::default();
    assert!(capabilities.experimental.is_none());
    assert!(capabilities.logging.is_none());
    assert!(capabilities.completions.is_none());
    assert!(capabilities.prompts.is_none());
    assert!(capabilities.resources.is_none());
    assert!(capabilities.tools.is_none());
}

#[test]
fn test_server_capabilities_with_values() {
    let capabilities = ServerCapabilities {
        experimental: None,
        logging: Some(LoggingCapabilities),
        completions: Some(CompletionCapabilities),
        prompts: Some(PromptsCapabilities {
            list_changed: Some(false),
        }),
        resources: Some(ResourcesCapabilities {
            subscribe: Some(true),
            list_changed: Some(true),
        }),
        tools: Some(ToolsCapabilities {
            list_changed: Some(false),
        }),
    };

    assert!(capabilities.logging.is_some());
    assert!(capabilities.completions.is_some());
    assert!(capabilities.prompts.is_some());
    assert!(capabilities.resources.is_some());
    assert!(capabilities.tools.is_some());
}

#[test]
fn test_capabilities_serialization() {
    let client_caps = ClientCapabilities::default();
    let server_caps = ServerCapabilities::default();

    let client_json = serde_json::to_string(&client_caps).unwrap();
    let server_json = serde_json::to_string(&server_caps).unwrap();

    let _client_deser: ClientCapabilities = serde_json::from_str(&client_json).unwrap();
    let _server_deser: ServerCapabilities = serde_json::from_str(&server_json).unwrap();
}

// ============================================================================
// Request/Response Tests
// ============================================================================

#[test]
fn test_initialize_request() {
    let request = InitializeRequest {
        protocol_version: "1.0.0".to_string(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "test-client".to_string(),
            title: None,
            version: "1.0.0".to_string(),
            #[cfg(feature = "mcp-draft")]
            description: None,
            #[cfg(feature = "mcp-icons")]
            icons: None,
        },
        _meta: None,
    };

    assert_eq!(request.protocol_version, "1.0.0");
    assert_eq!(request.client_info.name, "test-client");
}

#[test]
fn test_initialize_result() {
    let result = InitializeResult {
        protocol_version: "1.0.0".to_string(),
        capabilities: ServerCapabilities::default(),
        server_info: Implementation {
            name: "test-server".to_string(),
            title: Some("Test Server".to_string()),
            version: "1.0.0".to_string(),
            #[cfg(feature = "mcp-draft")]
            description: None,
            #[cfg(feature = "mcp-icons")]
            icons: None,
        },
        instructions: Some("Welcome to the server".to_string()),
        _meta: None,
    };

    assert_eq!(result.protocol_version, "1.0.0");
    assert_eq!(result.server_info.name, "test-server");
    assert_eq!(
        result.instructions,
        Some("Welcome to the server".to_string())
    );
}

#[test]
fn test_list_tools_request() {
    let request = ListToolsRequest::default();
    let json = serde_json::to_string(&request).unwrap();
    let _deserialized: ListToolsRequest = serde_json::from_str(&json).unwrap();
}

#[test]
fn test_list_tools_result() {
    let result = ListToolsResult {
        tools: vec![],
        next_cursor: Some("next".to_string()),
        _meta: None,
    };

    assert!(result.tools.is_empty());
    assert_eq!(result.next_cursor, Some("next".to_string()));
}

#[test]
fn test_call_tool_request() {
    let mut arguments = HashMap::new();
    arguments.insert("param1".to_string(), json!("value1"));

    let request = CallToolRequest {
        name: "test_tool".to_string(),
        arguments: Some(arguments),
        _meta: None,
    };

    assert_eq!(request.name, "test_tool");
    assert!(request.arguments.is_some());
}

#[test]
fn test_call_tool_result() {
    let content = vec![ContentBlock::Text(TextContent {
        text: "Tool result".to_string(),
        annotations: None,
        meta: None,
    })];

    let result = CallToolResult {
        content,
        is_error: Some(false),
        structured_content: None,
        _meta: None,
    };

    assert_eq!(result.content.len(), 1);
    assert_eq!(result.is_error, Some(false));
}

// ============================================================================
// Tool Tests
// ============================================================================

#[test]
fn test_tool() {
    let tool = Tool {
        name: "calculator".to_string(),
        title: Some("Calculator Tool".to_string()),
        description: Some("Performs calculations".to_string()),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
            additional_properties: None,
        },
        output_schema: None,
        execution: None,
        annotations: None,
        meta: None,
    };

    assert_eq!(tool.name, "calculator");
    assert_eq!(tool.title, Some("Calculator Tool".to_string()));
    assert_eq!(tool.input_schema.schema_type, "object");
}

#[test]
fn test_tool_with_annotations() {
    let annotations = ToolAnnotations {
        title: Some("Annotated Tool".to_string()),
        audience: Some(vec!["developers".to_string()]),
        priority: Some(1.0),
        custom: HashMap::new(),
        ..Default::default()
    };

    let tool = Tool {
        name: "annotated_tool".to_string(),
        title: None,
        description: None,
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
            additional_properties: None,
        },
        output_schema: None,
        annotations: Some(annotations),
        meta: None,
    };

    assert!(tool.annotations.is_some());
    if let Some(ref ann) = tool.annotations {
        assert_eq!(ann.title, Some("Annotated Tool".to_string()));
    }
}

#[test]
fn test_tool_input_schema() {
    let mut properties = HashMap::new();
    properties.insert("param1".to_string(), json!({"type": "string"}));

    let schema = ToolInputSchema {
        schema_type: "object".to_string(),
        properties: Some(properties),
        required: Some(vec!["param1".to_string()]),
        additional_properties: Some(false),
    };

    assert_eq!(schema.schema_type, "object");
    assert!(schema.properties.is_some());
    assert_eq!(schema.required, Some(vec!["param1".to_string()]));
    assert_eq!(schema.additional_properties, Some(false));
}

#[test]
fn test_tool_serialization() {
    let tool = Tool {
        name: "test".to_string(),
        title: None,
        description: None,
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
            additional_properties: None,
        },
        output_schema: None,
        execution: None,
        annotations: None,
        meta: None,
    };

    let json = serde_json::to_string(&tool).unwrap();
    let deserialized: Tool = serde_json::from_str(&json).unwrap();

    assert_eq!(tool.name, deserialized.name);
    assert_eq!(
        tool.input_schema.schema_type,
        deserialized.input_schema.schema_type
    );
}

// ============================================================================
// Resource Tests
// ============================================================================

#[test]
fn test_resource() {
    let resource = Resource {
        name: "test_resource".to_string(),
        title: Some("Test Resource".to_string()),
        uri: "file://test.txt".to_string(),
        description: Some("A test resource".to_string()),
        mime_type: Some("text/plain".to_string()),
        annotations: None,
        size: Some(1024),
        meta: None,
    };

    assert_eq!(resource.name, "test_resource");
    assert_eq!(resource.uri, "file://test.txt");
    assert_eq!(resource.size, Some(1024));
}

#[test]
fn test_text_resource_contents() {
    let contents = TextResourceContents {
        uri: "file://test.txt".to_string(),
        mime_type: Some("text/plain".to_string()),
        text: "Hello, World!".to_string(),
        meta: None,
    };

    assert_eq!(contents.text, "Hello, World!");
    assert_eq!(contents.mime_type, Some("text/plain".to_string()));
}

#[test]
fn test_blob_resource_contents() {
    let contents = BlobResourceContents {
        uri: "file://image.png".to_string(),
        mime_type: Some("image/png".to_string()),
        blob: "base64encodeddata".to_string(),
        meta: None,
    };

    assert_eq!(contents.blob, "base64encodeddata");
    assert_eq!(contents.mime_type, Some("image/png".to_string()));
}

#[test]
fn test_resource_content_variants() {
    let text_content = ResourceContent::Text(TextResourceContents {
        uri: "file://test.txt".to_string(),
        mime_type: Some("text/plain".to_string()),
        text: "Content".to_string(),
        meta: None,
    });

    let blob_content = ResourceContent::Blob(BlobResourceContents {
        uri: "file://image.png".to_string(),
        mime_type: Some("image/png".to_string()),
        blob: "data".to_string(),
        meta: None,
    });

    match text_content {
        ResourceContent::Text(contents) => assert_eq!(contents.text, "Content"),
        _ => panic!("Expected text content"),
    }

    match blob_content {
        ResourceContent::Blob(contents) => assert_eq!(contents.blob, "data"),
        _ => panic!("Expected blob content"),
    }
}

// ============================================================================
// Empty Types Tests
// ============================================================================

#[test]
fn test_empty_request_types() {
    let _list_tools = ListToolsRequest::default();
    let _list_prompts = ListPromptsRequest::default();
    let _list_roots = ListRootsRequest::default();
    let _initialized = InitializedNotification;
    let _set_level_result = SetLevelResult;
    let _roots_changed = RootsListChangedNotification;
}

#[test]
fn test_empty_result_default() {
    let result = EmptyResult::default();
    let json = serde_json::to_string(&result).unwrap();
    let _deserialized: EmptyResult = serde_json::from_str(&json).unwrap();
}

// ============================================================================
// Complex Type Tests
// ============================================================================

#[test]
fn test_client_request_variants() {
    let init_request = ClientRequest::Initialize(InitializeRequest {
        protocol_version: "1.0.0".to_string(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "client".to_string(),
            title: None,
            version: "1.0.0".to_string(),
            #[cfg(feature = "mcp-draft")]
            description: None,
            #[cfg(feature = "mcp-icons")]
            icons: None,
        },
        _meta: None,
    });

    let list_tools = ClientRequest::ListTools(ListToolsRequest::default());

    match init_request {
        ClientRequest::Initialize(req) => assert_eq!(req.protocol_version, "1.0.0"),
        _ => panic!("Expected initialize request"),
    }

    match list_tools {
        ClientRequest::ListTools(_) => (),
        _ => panic!("Expected list tools request"),
    }
}

#[test]
fn test_server_request_variants() {
    let ping = ServerRequest::Ping(PingParams::default());
    match ping {
        ServerRequest::Ping(_) => (),
        ServerRequest::CreateMessage(_) => (),
        ServerRequest::ListRoots(_) => (),
        ServerRequest::ElicitationCreate(_) => (),
    }
}

#[test]
fn test_client_notification_variants() {
    let initialized = ClientNotification::Initialized(InitializedNotification);

    match initialized {
        ClientNotification::Initialized(_) => (),
        _ => panic!("Expected initialized notification"),
    }
}

#[test]
fn test_include_context_variants() {
    let contexts = vec![
        IncludeContext::None,
        IncludeContext::ThisServer,
        IncludeContext::AllServers,
    ];

    for context in contexts {
        let json = serde_json::to_string(&context).unwrap();
        let _deserialized: IncludeContext = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn test_model_preferences() {
    let prefs = ModelPreferences {
        hints: Some(vec![ModelHint::new("fast")]),
        cost_priority: Some(1.0),         // High priority for low cost
        speed_priority: Some(1.0),        // High priority for speed
        intelligence_priority: Some(0.5), // Medium priority for intelligence
    };

    assert!(prefs.hints.is_some());
    assert_eq!(prefs.cost_priority, Some(1.0));
    assert_eq!(prefs.speed_priority, Some(1.0));
    assert_eq!(prefs.intelligence_priority, Some(0.5));
}

#[test]
fn test_comprehensive_serialization() {
    // Test a complex nested structure
    let mut meta = HashMap::new();
    meta.insert("custom_field".to_string(), json!("custom_value"));

    let tool = Tool {
        name: "complex_tool".to_string(),
        title: Some("Complex Tool".to_string()),
        description: Some("A complex tool for testing".to_string()),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "param1".to_string(),
                    json!({"type": "string", "description": "First parameter"}),
                );
                props.insert(
                    "param2".to_string(),
                    json!({"type": "integer", "minimum": 0}),
                );
                props
            }),
            required: Some(vec!["param1".to_string()]),
            additional_properties: Some(false),
        },
        output_schema: Some(ToolOutputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert("result".to_string(), json!({"type": "string"}));
                props
            }),
            required: Some(vec!["result".to_string()]),
            additional_properties: Some(false),
        }),
        annotations: Some(ToolAnnotations {
            title: Some("Annotated Complex Tool".to_string()),
            audience: Some(vec!["developers".to_string(), "testers".to_string()]),
            priority: Some(1.5),
            custom: {
                let mut custom = HashMap::new();
                custom.insert("category".to_string(), json!("utility"));
                custom
            },
            ..Default::default()
        }),
        meta: Some(meta),
    };

    let json = serde_json::to_string_pretty(&tool).unwrap();
    let deserialized: Tool = serde_json::from_str(&json).unwrap();

    assert_eq!(tool.name, deserialized.name);
    assert_eq!(tool.title, deserialized.title);
    assert_eq!(tool.description, deserialized.description);
    assert!(deserialized.annotations.is_some());
    assert!(deserialized.output_schema.is_some());
}

#[test]
fn test_sampling_api_comprehensive_workflow() {
    // Test complete CreateMessageRequest with all MCP 2025-06-18 fields
    let sampling_message = SamplingMessage {
        role: Role::User,
        content: Content::Text(TextContent {
            text: "Test message for sampling".to_string(),
            annotations: None,
            meta: None,
        }),
        metadata: None,
    };

    let model_preferences = ModelPreferences {
        hints: Some(vec![
            ModelHint::new("claude-3-5-sonnet"),
            ModelHint::new("fast"),
        ]),
        cost_priority: Some(1.0),         // High priority for low cost
        speed_priority: Some(1.0),        // High priority for speed
        intelligence_priority: Some(1.0), // High priority for intelligence
    };

    let mut metadata = HashMap::new();
    metadata.insert("provider".to_string(), json!("anthropic"));
    metadata.insert("region".to_string(), json!("us-east-1"));

    let create_message_request = CreateMessageRequest {
        messages: vec![sampling_message],
        model_preferences: Some(model_preferences.clone()),
        system_prompt: Some("You are a helpful assistant for testing.".to_string()),
        include_context: Some(IncludeContext::ThisServer),
        temperature: Some(0.7),
        max_tokens: 1000,
        stop_sequences: Some(vec!["STOP".to_string(), "END".to_string()]),
        _meta: None,
    };

    // Test serialization/deserialization
    let json = serde_json::to_string_pretty(&create_message_request).unwrap();
    let deserialized: CreateMessageRequest = serde_json::from_str(&json).unwrap();

    // Verify all fields are preserved
    assert_eq!(deserialized.messages.len(), 1);
    assert_eq!(deserialized.messages[0].role, Role::User);

    let model_prefs = deserialized.model_preferences.as_ref().unwrap();
    assert_eq!(model_prefs.cost_priority, Some(1.0));
    assert_eq!(model_prefs.speed_priority, Some(1.0));
    assert_eq!(model_prefs.intelligence_priority, Some(1.0));
    assert_eq!(model_prefs.hints.as_ref().unwrap().len(), 2);
    assert_eq!(
        model_prefs.hints.as_ref().unwrap()[0]
            .name
            .as_ref()
            .unwrap(),
        "claude-3-5-sonnet"
    );

    assert_eq!(
        deserialized.system_prompt,
        Some("You are a helpful assistant for testing.".to_string())
    );
    assert_eq!(
        deserialized.include_context,
        Some(IncludeContext::ThisServer)
    );
    assert_eq!(deserialized.temperature, Some(0.7));
    assert_eq!(deserialized.max_tokens, 1000);
    assert_eq!(deserialized.stop_sequences.as_ref().unwrap().len(), 2);
}

#[test]
fn test_sampling_api_context_inclusion_serialization() {
    // Test all IncludeContext variants serialize correctly per MCP spec
    let test_cases = vec![
        (IncludeContext::None, "\"none\""),
        (IncludeContext::ThisServer, "\"thisServer\""),
        (IncludeContext::AllServers, "\"allServers\""),
    ];

    for (context, expected_json) in test_cases {
        let json = serde_json::to_string(&context).unwrap();
        assert_eq!(json, expected_json);

        let deserialized: IncludeContext = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, context);
    }
}

#[test]
fn test_sampling_api_model_preferences_validation() {
    // Test priority value ranges per MCP spec (0.0 to 1.0)
    let valid_prefs = ModelPreferences {
        hints: None,
        cost_priority: Some(1.0),         // High priority for low cost
        speed_priority: Some(1.0),        // High priority for speed
        intelligence_priority: Some(0.5), // Medium priority for intelligence
    };

    let json = serde_json::to_string(&valid_prefs).unwrap();
    let deserialized: ModelPreferences = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.cost_priority, Some(1.0));
    assert_eq!(deserialized.speed_priority, Some(1.0));
    assert_eq!(deserialized.intelligence_priority, Some(0.5));
}

#[test]
fn test_create_message_result_complete() {
    // Test CreateMessageResult with all fields
    let result = CreateMessageResult {
        role: Role::Assistant,
        content: Content::Text(TextContent {
            text: "This is a test response from the model.".to_string(),
            annotations: None,
            meta: None,
        }),
        model: "claude-3-5-sonnet-20241022".to_string(),
        stop_reason: Some(StopReason::StopSequence),
        _meta: None,
    };

    let json = serde_json::to_string_pretty(&result).unwrap();
    let deserialized: CreateMessageResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.role, Role::Assistant);
    assert_eq!(deserialized.model, "claude-3-5-sonnet-20241022".to_string());
    assert_eq!(deserialized.stop_reason, Some(StopReason::StopSequence));

    if let Content::Text(text_content) = &deserialized.content {
        assert_eq!(text_content.text, "This is a test response from the model.");
    } else {
        panic!("Expected text content");
    }
}

#[test]
fn test_annotations_100_percent_compliance() {
    // Test all MCP 2025-06-18 Annotations fields for 100% schema compliance
    let annotations = Annotations {
        audience: Some(vec!["user".to_string(), "assistant".to_string()]),
        priority: Some(0.8),
        last_modified: Some("2025-01-12T15:00:58Z".to_string()),
        custom: {
            let mut custom = HashMap::new();
            custom.insert("category".to_string(), serde_json::json!("important"));
            custom.insert("source".to_string(), serde_json::json!("manual"));
            custom
        },
    };

    // Test serialization includes all fields with correct JSON naming
    let json = serde_json::to_string(&annotations).unwrap();
    assert!(json.contains("\"audience\":[\"user\",\"assistant\"]"));
    assert!(json.contains("\"priority\":0.8"));
    assert!(json.contains("\"lastModified\":\"2025-01-12T15:00:58Z\""));
    assert!(json.contains("\"category\":\"important\""));

    // Test round-trip serialization maintains all data
    let deserialized: Annotations = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.audience, annotations.audience);
    assert_eq!(deserialized.priority, annotations.priority);
    assert_eq!(deserialized.last_modified, annotations.last_modified);
    assert_eq!(deserialized.custom, annotations.custom);
}

#[test]
fn test_tool_annotations_100_percent_compliance() {
    // Test all MCP 2025-06-18 ToolAnnotations fields for 100% schema compliance
    let tool_annotations = ToolAnnotations {
        title: Some("Advanced File System Tool".to_string()),
        audience: Some(vec!["developer".to_string()]),
        priority: Some(1.0),
        destructive_hint: Some(true),
        idempotent_hint: Some(false),
        open_world_hint: Some(false),
        read_only_hint: Some(false),
        custom: {
            let mut custom = HashMap::new();
            custom.insert("requires_auth".to_string(), serde_json::json!(true));
            custom.insert("max_file_size".to_string(), serde_json::json!(1048576));
            custom
        },
    };

    // Test serialization includes all fields with correct JSON naming (camelCase hints)
    let json = serde_json::to_string(&tool_annotations).unwrap();
    assert!(json.contains("\"title\":\"Advanced File System Tool\""));
    assert!(json.contains("\"audience\":[\"developer\"]"));
    assert!(json.contains("\"priority\":1.0"));
    assert!(json.contains("\"destructiveHint\":true"));
    assert!(json.contains("\"idempotentHint\":false"));
    assert!(json.contains("\"openWorldHint\":false"));
    assert!(json.contains("\"readOnlyHint\":false"));
    assert!(json.contains("\"requires_auth\":true"));
    assert!(json.contains("\"max_file_size\":1048576"));

    // Test round-trip serialization maintains all data
    let deserialized: ToolAnnotations = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.title, tool_annotations.title);
    assert_eq!(deserialized.audience, tool_annotations.audience);
    assert_eq!(deserialized.priority, tool_annotations.priority);
    assert_eq!(
        deserialized.destructive_hint,
        tool_annotations.destructive_hint
    );
    assert_eq!(
        deserialized.idempotent_hint,
        tool_annotations.idempotent_hint
    );
    assert_eq!(
        deserialized.open_world_hint,
        tool_annotations.open_world_hint
    );
    assert_eq!(deserialized.read_only_hint, tool_annotations.read_only_hint);
    assert_eq!(deserialized.custom, tool_annotations.custom);
}

#[test]
fn test_tool_with_complete_annotations_integration() {
    // Integration test: Tool with complete ToolAnnotations demonstrates full MCP compliance
    let tool = Tool {
        name: "file_manager".to_string(),
        title: Some("File Manager Pro".to_string()),
        description: Some("Advanced file management operations".to_string()),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "path".to_string(),
                    serde_json::json!({"type": "string", "description": "File path"}),
                );
                props.insert("operation".to_string(), serde_json::json!({"type": "string", "enum": ["create", "read", "update", "delete"]}));
                props
            }),
            required: Some(vec!["path".to_string(), "operation".to_string()]),
            additional_properties: None,
        },
        output_schema: Some(ToolOutputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "success".to_string(),
                    serde_json::json!({"type": "boolean"}),
                );
                props.insert("message".to_string(), serde_json::json!({"type": "string"}));
                props
            }),
            required: None,
            additional_properties: None,
        }),
        annotations: Some(ToolAnnotations {
            title: Some("File Manager with Full Annotations".to_string()),
            audience: Some(vec!["developer".to_string(), "admin".to_string()]),
            priority: Some(0.9),
            destructive_hint: Some(true), // Can delete files
            idempotent_hint: Some(false), // Multiple calls may have different effects
            open_world_hint: Some(false), // Operates on closed file system
            read_only_hint: Some(false),  // Can modify file system
            custom: {
                let mut custom = HashMap::new();
                custom.insert("security_level".to_string(), serde_json::json!("high"));
                custom.insert("requires_confirmation".to_string(), serde_json::json!(true));
                custom
            },
        }),
        meta: Some({
            let mut meta = HashMap::new();
            meta.insert("version".to_string(), serde_json::json!("2.1.1"));
            meta.insert("last_updated".to_string(), serde_json::json!("2025-08-29"));
            meta
        }),
    };

    // Serialize tool with complete annotations
    let json = serde_json::to_string_pretty(&tool).unwrap();

    // Verify ToolAnnotations behavior hints are properly serialized
    assert!(json.contains("\"destructiveHint\": true"));
    assert!(json.contains("\"idempotentHint\": false"));
    assert!(json.contains("\"openWorldHint\": false"));
    assert!(json.contains("\"readOnlyHint\": false"));

    // Verify custom fields are preserved
    assert!(json.contains("\"security_level\": \"high\""));
    assert!(json.contains("\"requires_confirmation\": true"));

    // Test round-trip maintains all annotation data
    let deserialized: Tool = serde_json::from_str(&json).unwrap();
    let deserialized_annotations = deserialized.annotations.unwrap();
    assert_eq!(deserialized_annotations.destructive_hint, Some(true));
    assert_eq!(deserialized_annotations.idempotent_hint, Some(false));
    assert_eq!(deserialized_annotations.open_world_hint, Some(false));
    assert_eq!(deserialized_annotations.read_only_hint, Some(false));
}

// ============================================================================
// Clean API Tests (Improved Ergonomics)
// ============================================================================

#[test]
fn test_clean_tool_creation() {
    // Clean tool creation with minimal code!
    let tool = Tool::new("test_tool");

    assert_eq!(tool.name, "test_tool");
    assert_eq!(tool.input_schema.schema_type, "object");
    assert!(tool.description.is_none());
    assert!(tool.title.is_none());
    assert!(tool.annotations.is_none());
    assert!(tool.meta.is_none());
}

#[test]
fn test_clean_tool_with_description() {
    // Clean creation with helper method
    let tool = Tool::with_description("test_tool", "A test tool");

    assert_eq!(tool.name, "test_tool");
    assert_eq!(tool.description.as_deref(), Some("A test tool"));

    // 🎉 Only 3 lines of setup vs old 11+ lines!
}

#[test]
fn test_turbomcp_tool_definition_conversion_clean() {
    // Test clean tool creation patterns!
    let turbomcp_tool = Tool::with_description("test_tool", "A test tool");

    assert_eq!(turbomcp_tool.name, "test_tool");
    assert_eq!(turbomcp_tool.description, Some("A test tool".to_string()));

    // Success: Clean and simple API achieved!
}

#[test]
fn test_turbomcp_tool_definition_conversion_no_description() {
    // Clean test for tools without descriptions
    let turbomcp_tool = Tool::new("test_tool");

    assert_eq!(turbomcp_tool.name, "test_tool");
    assert_eq!(turbomcp_tool.description, None);

    // Defaults work perfectly!
}

#[test]
fn test_input_schema_defaults() {
    let schema = ToolInputSchema::default();

    assert_eq!(schema.schema_type, "object");
    assert!(schema.properties.is_none());
    assert!(schema.required.is_none());
    assert!(schema.additional_properties.is_none());
}

#[test]
fn test_input_schema_builder_methods() {
    // Test helper methods for common patterns
    let empty_schema = ToolInputSchema::empty();
    assert_eq!(empty_schema.schema_type, "object");

    let props = HashMap::from([
        ("x".to_string(), json!({"type": "number"})),
        ("y".to_string(), json!({"type": "number"})),
    ]);

    let schema_with_props = ToolInputSchema::with_properties(props.clone());
    assert_eq!(schema_with_props.properties, Some(props.clone()));

    let required_schema = ToolInputSchema::with_required_properties(
        props.clone(),
        vec!["x".to_string(), "y".to_string()],
    );
    assert_eq!(required_schema.properties, Some(props));
    assert_eq!(
        required_schema.required,
        Some(vec!["x".to_string(), "y".to_string()])
    );
}

#[test]
fn test_tool_name_validation() {
    // Test that Tool::new validates the name
    let tool = Tool::new("valid_name");
    assert_eq!(tool.name, "valid_name");

    // Test that Tool::with_description validates the name
    let tool_with_desc = Tool::with_description("valid_name", "description");
    assert_eq!(tool_with_desc.name, "valid_name");
    assert_eq!(tool_with_desc.description, Some("description".to_string()));
}

#[test]
#[should_panic(expected = "Tool name cannot be empty")]
fn test_tool_new_empty_name_panics() {
    Tool::new("");
}

#[test]
#[should_panic(expected = "Tool name cannot be empty")]
fn test_tool_new_whitespace_name_panics() {
    Tool::new("   ");
}

#[test]
#[should_panic(expected = "Tool name cannot be empty")]
fn test_tool_with_description_empty_name_panics() {
    Tool::with_description("", "description");
}

#[test]
fn test_tool_default_has_valid_name() {
    let tool = Tool::default();
    assert_eq!(tool.name, "unnamed_tool");
    assert!(!tool.name.trim().is_empty());
}

#[test]
fn verify_content_block_type_field_serialization() {
    use crate::types::{ContentBlock, TextContent};

    let text = ContentBlock::Text(TextContent {
        text: "Hello".to_string(),
        annotations: None,
        meta: None,
    });

    let json = serde_json::to_string_pretty(&text).unwrap();
    println!("\n=== TextContent JSON ===\n{}\n", json);

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("type").is_some(),
        "type field must exist in JSON"
    );
    assert_eq!(parsed["type"], "text", "type must be 'text'");
}

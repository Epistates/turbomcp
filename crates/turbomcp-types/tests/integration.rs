use turbomcp_types::{Tool, ToolInputSchema, ServerInfo, ToolResult};
use serde_json::json;

#[test]
fn test_tool_serialization() {
    let schema = ToolInputSchema {
        schema_type: "object".to_string(),
        properties: Some(json!({
            "arg": { "type": "string" }
        })),
        required: None,
        additional_properties: None,
    };

    let tool = Tool::new("test-tool", "A test tool")
        .with_schema(schema);

    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["name"], "test-tool");
    assert_eq!(json["description"], "A test tool");
    assert!(json["inputSchema"].is_object());
}

#[test]
fn test_result_builders() {
    let text_result = ToolResult::text("Hello world");
    assert!(!text_result.is_error());
    
    let error_result = ToolResult::error("Failure");
    assert!(error_result.is_error());
}

#[test]
fn test_server_info() {
    let info = ServerInfo::new("my-server", "1.0.0");
    assert_eq!(info.name, "my-server");
    assert_eq!(info.version, "1.0.0");
}

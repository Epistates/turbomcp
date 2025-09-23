//! Comprehensive tests for the registry module

use serde_json::json;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use turbomcp::registry::{
    HandlerRegistry, PromptRegistration, PromptRequest, ResourceRegistration, ResourceRequest,
    ToolRegistration, ToolRequest,
};
use turbomcp::{CallToolResult, McpResult};
use turbomcp_core::RequestContext;
use turbomcp_protocol::types::{
    ContentBlock, GetPromptResult, ReadResourceResult, ResourceContent, TextContent,
    TextResourceContents,
};

// Helper function to create a dummy tool handler
fn dummy_tool_handler(
    _server: &dyn std::any::Any,
    _request: ToolRequest,
) -> Pin<Box<dyn Future<Output = McpResult<CallToolResult>> + Send>> {
    Box::pin(async move {
        Ok(CallToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: "dummy result".to_string(),
                annotations: None,
                meta: None,
            })],
            is_error: Some(false),
            structured_content: None,
            _meta: None,
        })
    })
}

// Helper function to create a dummy resource handler
fn dummy_resource_handler(
    _server: &dyn std::any::Any,
    _request: ResourceRequest,
) -> Pin<Box<dyn Future<Output = McpResult<ReadResourceResult>> + Send>> {
    Box::pin(async move {
        Ok(ReadResourceResult {
            contents: vec![ResourceContent::Text(TextResourceContents {
                uri: "dummy://resource".to_string(),
                mime_type: Some("text/plain".to_string()),
                text: "dummy resource".to_string(),
                meta: None,
            })],
            _meta: None,
        })
    })
}

// Helper function to create a dummy prompt handler
fn dummy_prompt_handler(
    _server: &dyn std::any::Any,
    _request: PromptRequest,
) -> Pin<Box<dyn Future<Output = McpResult<GetPromptResult>> + Send>> {
    Box::pin(async move {
        Ok(GetPromptResult {
            description: Some("dummy prompt".to_string()),
            messages: vec![],
            _meta: None,
        })
    })
}

#[test]
fn test_handler_registry_creation() {
    let registry = HandlerRegistry::new();

    // Registry should be created successfully
    // The actual content depends on whether any handlers are registered via inventory
    // but we can test that the methods work
    let _tools = registry.tools();
    let _resources = registry.resources();
    let _prompts = registry.prompts();
}

#[test]
fn test_handler_registry_default() {
    let registry = HandlerRegistry::default();

    // Should be equivalent to new()
    let _tools = registry.tools();
    let _resources = registry.resources();
    let _prompts = registry.prompts();
}

#[test]
fn test_tool_registration_structure() {
    let registration = ToolRegistration {
        name: "test_tool",
        description: "A test tool",
        schema: Some(json!({"type": "object", "properties": {}})),
        allowed_roles: Some(&["admin", "user"]),
        handler: dummy_tool_handler,
    };

    assert_eq!(registration.name, "test_tool");
    assert_eq!(registration.description, "A test tool");
    assert!(registration.schema.is_some());
    assert!(registration.allowed_roles.is_some());
    assert_eq!(registration.allowed_roles.unwrap().len(), 2);
    assert_eq!(registration.allowed_roles.unwrap()[0], "admin");
    assert_eq!(registration.allowed_roles.unwrap()[1], "user");
}

#[test]
fn test_tool_registration_no_schema_no_roles() {
    let registration = ToolRegistration {
        name: "simple_tool",
        description: "A simple tool",
        schema: None,
        allowed_roles: None,
        handler: dummy_tool_handler,
    };

    assert_eq!(registration.name, "simple_tool");
    assert_eq!(registration.description, "A simple tool");
    assert!(registration.schema.is_none());
    assert!(registration.allowed_roles.is_none());
}

#[test]
fn test_resource_registration_structure() {
    let registration = ResourceRegistration {
        name: "test_resource",
        description: "A test resource",
        uri_template: Some("file://{path}"),
        handler: dummy_resource_handler,
    };

    assert_eq!(registration.name, "test_resource");
    assert_eq!(registration.description, "A test resource");
    assert_eq!(registration.uri_template.unwrap(), "file://{path}");
}

#[test]
fn test_resource_registration_no_uri_template() {
    let registration = ResourceRegistration {
        name: "simple_resource",
        description: "A simple resource",
        uri_template: None,
        handler: dummy_resource_handler,
    };

    assert_eq!(registration.name, "simple_resource");
    assert_eq!(registration.description, "A simple resource");
    assert!(registration.uri_template.is_none());
}

#[test]
fn test_prompt_registration_structure() {
    let registration = PromptRegistration {
        name: "test_prompt",
        description: "A test prompt",
        handler: dummy_prompt_handler,
    };

    assert_eq!(registration.name, "test_prompt");
    assert_eq!(registration.description, "A test prompt");
}

#[test]
fn test_tool_request_structure() {
    let context = RequestContext::new().with_session_id("test_session");

    let mut arguments = HashMap::new();
    arguments.insert("param1".to_string(), json!("value1"));
    arguments.insert("param2".to_string(), json!(42));

    let request = ToolRequest { context, arguments };

    assert_eq!(request.context.session_id.as_ref().unwrap(), "test_session");
    assert_eq!(request.arguments.len(), 2);
    assert_eq!(request.arguments.get("param1").unwrap(), &json!("value1"));
    assert_eq!(request.arguments.get("param2").unwrap(), &json!(42));
}

#[test]
fn test_resource_request_structure() {
    let context = RequestContext::new().with_session_id("test_session");

    let mut parameters = HashMap::new();
    parameters.insert("path".to_string(), "/home/user/file.txt".to_string());
    parameters.insert("format".to_string(), "json".to_string());

    let request = ResourceRequest {
        context,
        uri: "file:///home/user/file.txt".to_string(),
        parameters,
    };

    assert_eq!(request.context.session_id.as_ref().unwrap(), "test_session");
    assert_eq!(request.uri, "file:///home/user/file.txt");
    assert_eq!(request.parameters.len(), 2);
    assert_eq!(
        request.parameters.get("path").unwrap(),
        "/home/user/file.txt"
    );
    assert_eq!(request.parameters.get("format").unwrap(), "json");
}

#[test]
fn test_prompt_request_structure() {
    let context = RequestContext::new().with_session_id("test_session");

    let mut arguments = HashMap::new();
    arguments.insert("topic".to_string(), json!("rust programming"));
    arguments.insert("level".to_string(), json!("beginner"));

    let request = PromptRequest { context, arguments };

    assert_eq!(request.context.session_id.as_ref().unwrap(), "test_session");
    assert_eq!(request.arguments.len(), 2);
    assert_eq!(
        request.arguments.get("topic").unwrap(),
        &json!("rust programming")
    );
    assert_eq!(request.arguments.get("level").unwrap(), &json!("beginner"));
}

#[tokio::test]
async fn test_tool_handler_execution() {
    let context = RequestContext::new().with_session_id("test_session");

    let request = ToolRequest {
        context,
        arguments: HashMap::new(),
    };

    let result = dummy_tool_handler(&(), request).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert_eq!(call_result.is_error, Some(false));
    assert_eq!(call_result.content.len(), 1);

    if let ContentBlock::Text(text_content) = &call_result.content[0] {
        assert_eq!(text_content.text, "dummy result");
    } else {
        panic!("Expected text content");
    }
}

#[tokio::test]
async fn test_resource_handler_execution() {
    let context = RequestContext::new().with_session_id("test_session");

    let request = ResourceRequest {
        context,
        uri: "test://resource".to_string(),
        parameters: HashMap::new(),
    };

    let result = dummy_resource_handler(&(), request).await;

    assert!(result.is_ok());
    let resource_result = result.unwrap();
    assert_eq!(resource_result.contents.len(), 1);

    if let ResourceContent::Text(text_content) = &resource_result.contents[0] {
        assert_eq!(text_content.text, "dummy resource");
    } else {
        panic!("Expected text resource content");
    }
}

#[tokio::test]
async fn test_prompt_handler_execution() {
    let context = RequestContext::new().with_session_id("test_session");

    let request = PromptRequest {
        context,
        arguments: HashMap::new(),
    };

    let result = dummy_prompt_handler(&(), request).await;

    assert!(result.is_ok());
    let prompt_result = result.unwrap();
    assert_eq!(prompt_result.description.as_ref().unwrap(), "dummy prompt");
    assert!(prompt_result.messages.is_empty());
}

#[test]
fn test_registry_find_methods_empty() {
    let registry = HandlerRegistry::new();

    // In a clean test environment, these should return None
    // since we don't have any registered handlers via inventory
    let tool = registry.find_tool("nonexistent_tool");
    let resource = registry.find_resource("nonexistent_resource");
    let prompt = registry.find_prompt("nonexistent_prompt");

    assert!(tool.is_none());
    assert!(resource.is_none());
    assert!(prompt.is_none());
}

#[test]
fn test_registry_collections_consistency() {
    let registry = HandlerRegistry::new();

    // Test that the collections are consistent
    let tools = registry.tools();
    let resources = registry.resources();
    let prompts = registry.prompts();

    // All should be slices (this tests the return types are accessible)
    let _tool_count = tools.len(); // Could be 0 or more
    let _resource_count = resources.len();
    let _prompt_count = prompts.len();

    // Test that calling multiple times gives consistent results
    let tools2 = registry.tools();
    let resources2 = registry.resources();
    let prompts2 = registry.prompts();

    assert_eq!(tools.len(), tools2.len());
    assert_eq!(resources.len(), resources2.len());
    assert_eq!(prompts.len(), prompts2.len());
}

// Test with complex request contexts
#[test]
fn test_request_with_complex_context() {
    let context = RequestContext::new()
        .with_session_id("complex_session")
        .with_metadata("user_id", "123")
        .with_metadata("role", "admin");

    let tool_request = ToolRequest {
        context: context.clone(),
        arguments: HashMap::new(),
    };

    let resource_request = ResourceRequest {
        context: context.clone(),
        uri: "complex://resource".to_string(),
        parameters: HashMap::new(),
    };

    let prompt_request = PromptRequest {
        context,
        arguments: HashMap::new(),
    };

    // All should be constructible with complex contexts
    assert_eq!(
        tool_request.context.session_id.as_ref().unwrap(),
        "complex_session"
    );
    assert_eq!(
        resource_request.context.session_id.as_ref().unwrap(),
        "complex_session"
    );
    assert_eq!(
        prompt_request.context.session_id.as_ref().unwrap(),
        "complex_session"
    );
}

// Test with empty collections
#[test]
fn test_empty_arguments_and_parameters() {
    let context = RequestContext::new().with_session_id("empty_test");

    let tool_request = ToolRequest {
        context: context.clone(),
        arguments: HashMap::new(),
    };

    let resource_request = ResourceRequest {
        context: context.clone(),
        uri: "empty://resource".to_string(),
        parameters: HashMap::new(),
    };

    let prompt_request = PromptRequest {
        context,
        arguments: HashMap::new(),
    };

    assert!(tool_request.arguments.is_empty());
    assert!(resource_request.parameters.is_empty());
    assert!(prompt_request.arguments.is_empty());
}

// Test allowed_roles with different configurations
#[test]
fn test_tool_registration_role_variations() {
    // No roles specified
    let no_roles = ToolRegistration {
        name: "no_roles",
        description: "Tool with no role restrictions",
        schema: None,
        allowed_roles: None,
        handler: dummy_tool_handler,
    };

    // Empty roles array
    let empty_roles = ToolRegistration {
        name: "empty_roles",
        description: "Tool with empty roles",
        schema: None,
        allowed_roles: Some(&[]),
        handler: dummy_tool_handler,
    };

    // Single role
    let single_role = ToolRegistration {
        name: "single_role",
        description: "Tool with single role",
        schema: None,
        allowed_roles: Some(&["admin"]),
        handler: dummy_tool_handler,
    };

    // Multiple roles
    let multiple_roles = ToolRegistration {
        name: "multiple_roles",
        description: "Tool with multiple roles",
        schema: None,
        allowed_roles: Some(&["admin", "user", "guest"]),
        handler: dummy_tool_handler,
    };

    assert!(no_roles.allowed_roles.is_none());
    assert_eq!(empty_roles.allowed_roles.unwrap().len(), 0);
    assert_eq!(single_role.allowed_roles.unwrap().len(), 1);
    assert_eq!(single_role.allowed_roles.unwrap()[0], "admin");
    assert_eq!(multiple_roles.allowed_roles.unwrap().len(), 3);
    assert!(multiple_roles.allowed_roles.unwrap().contains(&"admin"));
    assert!(multiple_roles.allowed_roles.unwrap().contains(&"user"));
    assert!(multiple_roles.allowed_roles.unwrap().contains(&"guest"));
}

// Test JSON schema variations
#[test]
fn test_tool_registration_schema_variations() {
    // Complex schema
    let complex_schema = ToolRegistration {
        name: "complex_tool",
        description: "Tool with complex schema",
        schema: Some(json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "User name"
                },
                "age": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 120
                },
                "preferences": {
                    "type": "object",
                    "properties": {
                        "theme": {"type": "string"},
                        "notifications": {"type": "boolean"}
                    }
                }
            },
            "required": ["name"]
        })),
        allowed_roles: None,
        handler: dummy_tool_handler,
    };

    // Simple schema
    let simple_schema = ToolRegistration {
        name: "simple_tool",
        description: "Tool with simple schema",
        schema: Some(json!({
            "type": "object",
            "properties": {
                "input": {"type": "string"}
            }
        })),
        allowed_roles: None,
        handler: dummy_tool_handler,
    };

    assert!(complex_schema.schema.is_some());
    assert!(simple_schema.schema.is_some());

    let complex_schema_value = complex_schema.schema.unwrap();
    let simple_schema_value = simple_schema.schema.unwrap();
    let complex_obj = complex_schema_value.as_object().unwrap();
    let simple_obj = simple_schema_value.as_object().unwrap();

    assert_eq!(complex_obj.get("type").unwrap().as_str().unwrap(), "object");
    assert_eq!(simple_obj.get("type").unwrap().as_str().unwrap(), "object");

    let complex_props = complex_obj.get("properties").unwrap().as_object().unwrap();
    let simple_props = simple_obj.get("properties").unwrap().as_object().unwrap();

    assert_eq!(complex_props.len(), 3); // name, age, preferences
    assert_eq!(simple_props.len(), 1); // input
}

// Test URI template variations
#[test]
fn test_resource_registration_uri_variations() {
    let file_resource = ResourceRegistration {
        name: "file_resource",
        description: "File system resource",
        uri_template: Some("file://{path}"),
        handler: dummy_resource_handler,
    };

    let http_resource = ResourceRegistration {
        name: "http_resource",
        description: "HTTP resource",
        uri_template: Some("https://api.example.com/{endpoint}"),
        handler: dummy_resource_handler,
    };

    let database_resource = ResourceRegistration {
        name: "database_resource",
        description: "Database resource",
        uri_template: Some("db://{table}/{id}"),
        handler: dummy_resource_handler,
    };

    assert_eq!(file_resource.uri_template.unwrap(), "file://{path}");
    assert_eq!(
        http_resource.uri_template.unwrap(),
        "https://api.example.com/{endpoint}"
    );
    assert_eq!(database_resource.uri_template.unwrap(), "db://{table}/{id}");
}

// Test that registry methods can be called multiple times safely
#[test]
fn test_registry_method_safety() {
    let registry = HandlerRegistry::new();

    // Call methods multiple times
    for _ in 0..5 {
        let _tools = registry.tools();
        let _resources = registry.resources();
        let _prompts = registry.prompts();

        let _tool = registry.find_tool("test");
        let _resource = registry.find_resource("test");
        let _prompt = registry.find_prompt("test");
    }

    // Should not panic or cause issues
}

// Test request structure with various data types
#[test]
fn test_request_arguments_data_types() {
    let context = RequestContext::new().with_session_id("data_types_test");

    let mut arguments = HashMap::new();
    arguments.insert("string".to_string(), json!("hello"));
    arguments.insert("number".to_string(), json!(42));
    arguments.insert("float".to_string(), json!(std::f64::consts::PI));
    arguments.insert("boolean".to_string(), json!(true));
    arguments.insert("array".to_string(), json!([1, 2, 3]));
    arguments.insert("object".to_string(), json!({"key": "value"}));
    arguments.insert("null".to_string(), json!(null));

    let request = ToolRequest { context, arguments };

    assert_eq!(request.arguments.len(), 7);
    assert_eq!(
        request.arguments.get("string").unwrap().as_str().unwrap(),
        "hello"
    );
    assert_eq!(
        request.arguments.get("number").unwrap().as_i64().unwrap(),
        42
    );
    assert_eq!(
        request.arguments.get("float").unwrap().as_f64().unwrap(),
        std::f64::consts::PI
    );
    assert!(request.arguments.get("boolean").unwrap().as_bool().unwrap());
    assert!(request.arguments.get("array").unwrap().is_array());
    assert!(request.arguments.get("object").unwrap().is_object());
    assert!(request.arguments.get("null").unwrap().is_null());
}

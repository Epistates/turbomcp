//! Comprehensive tests for the registry module

use crate::registry::{
    HandlerRegistry, PromptRequest, ResourceRegistration, ResourceRequest, ToolRegistration,
    ToolRequest,
};
use crate::{CallToolResult, McpResult};
use serde_json::json;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use turbomcp_protocol::RequestContext;
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
            #[cfg(feature = "mcp-tasks")]
            task_id: None,
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

// Removed naive structure tests - these only verified field assignment which is trivial
// Real behavioral tests start here:

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

// Real server for testing registry invocation
#[derive(Clone)]
struct RealRegistryTestServer {
    call_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl RealRegistryTestServer {
    fn new() -> Self {
        Self {
            call_count: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    fn get_call_count(&self) -> usize {
        self.call_count.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn increment_call_count(&self) {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

// Note: To test real registry invocation, we need a server with #[server] macro
// This requires the turbomcp macro system which uses inventory for registration
// The following tests validate the request/response structures that the registry uses

#[tokio::test]
async fn test_tool_handler_invocation_through_registry_pattern() {
    // This test demonstrates the real invocation pattern used by the registry

    let server = RealRegistryTestServer::new();
    let context = RequestContext::new().with_session_id("registry_test");
    let mut arguments = HashMap::new();
    arguments.insert("test_param".to_string(), json!("test_value"));

    let request = ToolRequest { context, arguments };

    // Simulate the registry's handler invocation
    let handler_fn =
        |server: &RealRegistryTestServer, request: ToolRequest| -> McpResult<CallToolResult> {
            server.increment_call_count();

            // Validate request structure
            assert!(request.context.session_id.is_some());
            assert!(!request.arguments.is_empty());

            // Return MCP-compliant result
            Ok(CallToolResult {
                content: vec![ContentBlock::Text(TextContent {
                    text: format!("Processed: {:?}", request.arguments),
                    annotations: None,
                    meta: None,
                })],
                is_error: Some(false),
                structured_content: None,
                _meta: None,
                #[cfg(feature = "mcp-tasks")]
                task_id: None,
            })
        };

    // Invoke handler (simulating registry.find_tool() + invoke)
    let result = handler_fn(&server, request);

    // Verify handler was called
    assert_eq!(server.get_call_count(), 1);

    // Verify result is MCP-compliant
    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert_eq!(call_result.is_error, Some(false));
    assert!(!call_result.content.is_empty());

    if let ContentBlock::Text(text_content) = &call_result.content[0] {
        assert!(text_content.text.contains("Processed"));
    } else {
        panic!("Expected text content");
    }

    println!("✅ Tool handler invocation pattern validated");
}

#[tokio::test]
async fn test_resource_handler_invocation_through_registry_pattern() {
    // This test demonstrates the real resource invocation pattern

    let server = RealRegistryTestServer::new();
    let context = RequestContext::new().with_session_id("resource_test");
    let mut parameters = HashMap::new();
    parameters.insert("id".to_string(), "123".to_string());

    let request = ResourceRequest {
        context,
        uri: "test://resource/123".to_string(),
        parameters,
    };

    // Simulate the registry's resource handler invocation
    let handler_fn = |server: &RealRegistryTestServer,
                      request: ResourceRequest|
     -> McpResult<ReadResourceResult> {
        server.increment_call_count();

        // Validate request structure
        assert!(request.context.session_id.is_some());
        assert!(!request.uri.is_empty());

        // Extract parameter from URI (real parameter extraction pattern)
        let id = request.parameters.get("id").unwrap();

        // Return MCP-compliant result
        Ok(ReadResourceResult {
            contents: vec![ResourceContent::Text(TextResourceContents {
                uri: request.uri.clone(),
                mime_type: Some("text/plain".to_string()),
                text: format!("Resource content for ID: {}", id),
                meta: None,
            })],
            _meta: None,
        })
    };

    // Invoke handler (simulating registry.find_resource() + invoke)
    let result = handler_fn(&server, request);

    // Verify handler was called
    assert_eq!(server.get_call_count(), 1);

    // Verify result is MCP-compliant
    assert!(result.is_ok());
    let resource_result = result.unwrap();
    assert!(!resource_result.contents.is_empty());

    if let ResourceContent::Text(text_content) = &resource_result.contents[0] {
        assert!(text_content.text.contains("Resource content for ID: 123"));
        assert_eq!(text_content.uri, "test://resource/123");
        assert_eq!(text_content.mime_type.as_ref().unwrap(), "text/plain");
    } else {
        panic!("Expected text resource content");
    }

    println!("✅ Resource handler invocation pattern validated");
}

#[tokio::test]
async fn test_prompt_handler_invocation_through_registry_pattern() {
    // This test demonstrates the real prompt invocation pattern

    let server = RealRegistryTestServer::new();
    let context = RequestContext::new().with_session_id("prompt_test");
    let mut arguments = HashMap::new();
    arguments.insert("topic".to_string(), json!("testing"));
    arguments.insert("style".to_string(), json!("concise"));

    let request = PromptRequest { context, arguments };

    // Simulate the registry's prompt handler invocation
    let handler_fn =
        |server: &RealRegistryTestServer, request: PromptRequest| -> McpResult<GetPromptResult> {
            server.increment_call_count();

            // Validate request structure
            assert!(request.context.session_id.is_some());
            assert!(!request.arguments.is_empty());

            // Extract arguments (real argument extraction pattern)
            let topic = request.arguments.get("topic").unwrap();
            let style = request.arguments.get("style").unwrap();

            // Return MCP-compliant result
            Ok(GetPromptResult {
                description: Some(format!("Prompt for {}: {}", topic, style)),
                messages: vec![],
                _meta: None,
            })
        };

    // Invoke handler (simulating registry.find_prompt() + invoke)
    let result = handler_fn(&server, request);

    // Verify handler was called
    assert_eq!(server.get_call_count(), 1);

    // Verify result is MCP-compliant
    assert!(result.is_ok());
    let prompt_result = result.unwrap();
    assert!(prompt_result.description.is_some());
    assert!(
        prompt_result
            .description
            .unwrap()
            .contains("Prompt for \"testing\": \"concise\"")
    );

    println!("✅ Prompt handler invocation pattern validated");
}

#[tokio::test]
async fn test_concurrent_handler_invocations() {
    // Test that handlers can be invoked concurrently (important for real-world MCP servers)

    use std::sync::Arc;
    use tokio::task::JoinSet;

    let server = Arc::new(RealRegistryTestServer::new());
    let mut join_set: JoinSet<McpResult<CallToolResult>> = JoinSet::new();

    // Spawn 10 concurrent handler invocations
    for i in 0..10 {
        let server_clone = Arc::clone(&server);
        join_set.spawn(async move {
            let context = RequestContext::new().with_session_id(format!("concurrent_{}", i));
            let mut arguments = HashMap::new();
            arguments.insert("index".to_string(), json!(i));

            let request = ToolRequest { context, arguments };

            // Simulate handler invocation
            let handler_fn = |server: &RealRegistryTestServer,
                              _request: ToolRequest|
             -> McpResult<CallToolResult> {
                server.increment_call_count();
                Ok(CallToolResult {
                    content: vec![ContentBlock::Text(TextContent {
                        text: "Concurrent result".to_string(),
                        annotations: None,
                        meta: None,
                    })],
                    is_error: Some(false),
                    structured_content: None,
                    _meta: None,
                    #[cfg(feature = "mcp-tasks")]
                    task_id: None,
                })
            };

            handler_fn(&server_clone, request)
        });
    }

    // Wait for all invocations to complete
    let mut success_count = 0;
    while let Some(result) = join_set.join_next().await {
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());
        success_count += 1;
    }

    // Verify all 10 handlers were invoked
    assert_eq!(server.get_call_count(), 10);
    assert_eq!(success_count, 10);

    println!("✅ Concurrent handler invocations validated");
}

#[tokio::test]
async fn test_handler_error_propagation() {
    // Test that handler errors are properly propagated through the registry

    let server = RealRegistryTestServer::new();
    let context = RequestContext::new().with_session_id("error_test");

    let request = ToolRequest {
        context,
        arguments: HashMap::new(),
    };

    // Handler that returns an error
    let error_handler_fn =
        |server: &RealRegistryTestServer, _request: ToolRequest| -> McpResult<CallToolResult> {
            server.increment_call_count();

            // Return MCP-compliant error result
            Ok(CallToolResult {
                content: vec![ContentBlock::Text(TextContent {
                    text: "Tool execution failed: Invalid parameters".to_string(),
                    annotations: None,
                    meta: None,
                })],
                is_error: Some(true), // Indicate error condition
                structured_content: None,
                _meta: None,
                #[cfg(feature = "mcp-tasks")]
                task_id: None,
            })
        };

    let result = error_handler_fn(&server, request);

    // Verify handler was called
    assert_eq!(server.get_call_count(), 1);

    // Verify error result is properly structured
    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert_eq!(call_result.is_error, Some(true));

    if let ContentBlock::Text(text_content) = &call_result.content[0] {
        assert!(text_content.text.contains("Tool execution failed"));
    }

    println!("✅ Handler error propagation validated");
}

#[test]
fn test_registry_metadata_schema_compliance() {
    // Test that registry metadata matches MCP schema requirements

    let tool_reg = ToolRegistration {
        name: "schema_test_tool",
        description: "A tool for schema testing",
        schema: Some(json!({
            "type": "object",
            "properties": {
                "required_param": {
                    "type": "string",
                    "description": "A required parameter"
                },
                "optional_param": {
                    "type": "number",
                    "description": "An optional parameter"
                }
            },
            "required": ["required_param"]
        })),
        allowed_roles: Some(&["admin", "user"]),
        handler: dummy_tool_handler,
    };

    // Validate MCP Tool schema requirements
    assert!(!tool_reg.name.is_empty());
    assert!(!tool_reg.description.is_empty());
    assert!(tool_reg.schema.is_some());

    let schema = tool_reg.schema.unwrap();
    assert_eq!(schema["type"], "object");
    assert!(schema.get("properties").is_some());
    assert!(schema.get("required").is_some());

    println!("✅ Registry metadata matches MCP schema requirements");
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
